//! Rattache un dossier de travail à son dépôt en lisant `.git` sur le disque :
//! un dossier `.git` marque l'arbre principal, un fichier `.git` (`gitdir: …`) un worktree lié.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use robiade_core::project::{repository_of_gitdir, Workspace};

#[derive(Default)]
pub struct WorkspaceResolver {
    /// La nature d'un dossier ne change pas pendant une session : on ne lit le disque qu'une fois.
    cache: Mutex<HashMap<String, Workspace>>,
}

impl WorkspaceResolver {
    pub fn resolve(&self, path: &str) -> Workspace {
        if let Some(found) = self.cache.lock().ok().and_then(|c| c.get(path).cloned()) {
            return found;
        }
        let found = locate(Path::new(path)).unwrap_or_else(|| Workspace::outside_git(path));
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(path.to_owned(), found.clone());
        }
        found
    }
}

/// Remonte depuis `path` jusqu'au premier `.git` : un sous-dossier appartient à son dépôt.
fn locate(path: &Path) -> Option<Workspace> {
    let top = path.ancestors().find(|dir| dir.join(".git").exists())?;
    let dot_git = top.join(".git");
    let top_str = top.to_string_lossy().into_owned();
    if dot_git.is_dir() {
        return Some(Workspace { project_root: top_str, worktree: None, is_repository: true });
    }
    let content = std::fs::read_to_string(&dot_git).ok()?;
    let gitdir = content.strip_prefix("gitdir:")?.trim();
    let repository = repository_of_gitdir(gitdir)?;
    Some(Workspace {
        project_root: repository.to_owned(),
        worktree: Some(top_str),
        is_repository: Path::new(repository).join(".git").is_dir(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_the_main_tree_from_a_linked_worktree() {
        let base = std::env::temp_dir().join(format!("robiade-ws-{}", std::process::id()));
        let repo = base.join("repo");
        let wt = base.join("repo-wt-fix");
        std::fs::create_dir_all(repo.join(".git/worktrees/repo-wt-fix")).unwrap();
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", repo.join(".git/worktrees/repo-wt-fix").display()))
            .unwrap();
        let resolver = WorkspaceResolver::default();
        let repo_str = repo.to_string_lossy().into_owned();

        let main = resolver.resolve(&repo.join("sub").to_string_lossy());
        assert_eq!((main.project_root.as_str(), main.worktree, main.is_repository), (repo_str.as_str(), None, true));

        let linked = resolver.resolve(&wt.to_string_lossy());
        assert_eq!(linked.project_root, repo_str);
        assert_eq!(linked.worktree_name(), Some("repo-wt-fix"));
        assert!(linked.is_repository);

        let gone = resolver.resolve(&base.join("disparu").to_string_lossy());
        assert!(!gone.is_repository);
        std::fs::remove_dir_all(base).unwrap();
    }
}
