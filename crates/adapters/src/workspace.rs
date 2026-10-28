//! Rattache un dossier de travail à son dépôt en lisant `.git` sur le disque :
//! un dossier `.git` marque l'arbre principal, un fichier `.git` (`gitdir: …`) un worktree lié.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use robiade_core::project::{gitlab_project_of_remote, repository_of_gitdir, Workspace};

#[derive(Default)]
pub struct WorkspaceResolver {
    /// La nature d'un dossier ne change pas pendant une session, sauf quand Robiade supprime un
    /// worktree (`forget`) : on ne lit le disque qu'une fois.
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

    /// Oublie `dir` et ses sous-dossiers : ils seront relus sur le disque.
    pub fn forget(&self, dir: &str) {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        if let Ok(mut cache) = self.cache.lock() {
            cache.retain(|path, _| path != dir && !path.starts_with(&prefix));
        }
    }
}

/// Remonte depuis `path` jusqu'au premier `.git` : un sous-dossier appartient à son dépôt.
fn locate(path: &Path) -> Option<Workspace> {
    let top = path.ancestors().find(|dir| dir.join(".git").exists())?;
    let dot_git = top.join(".git");
    let top_str = top.to_string_lossy().into_owned();
    if dot_git.is_dir() {
        let gitlab_project = gitlab_project_of(top);
        return Some(Workspace { project_root: top_str, worktree: None, is_repository: true, gitlab_project });
    }
    let content = std::fs::read_to_string(&dot_git).ok()?;
    let gitdir = content.strip_prefix("gitdir:")?.trim();
    let repository = repository_of_gitdir(gitdir)?;
    Some(Workspace {
        project_root: repository.to_owned(),
        worktree: Some(top_str),
        is_repository: Path::new(repository).join(".git").is_dir(),
        gitlab_project: gitlab_project_of(Path::new(repository)),
    })
}

/// Projet GitLab du dépôt, lu dans `.git/config` sans lancer git : rien de ce que la config déclare
/// ne s'exécute. Les `include` ne sont pas suivis ; un `origin` déclaré là n'est pas vu.
fn gitlab_project_of(repository: &Path) -> Option<String> {
    let config = std::fs::read_to_string(repository.join(".git").join("config")).ok()?;
    gitlab_project_of_remote(&origin_url(&config)?)
}

/// `url` de la section `[remote "origin"]` d'une config Git.
fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line.split_whitespace().collect::<Vec<_>>() == ["[remote", "\"origin\"]"];
            continue;
        }
        if !in_origin {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim().eq_ignore_ascii_case("url") {
                return Some(value.trim().trim_matches('"').to_owned());
            }
        }
    }
    None
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
        std::fs::write(
            repo.join(".git/config"),
            "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = git@gitlab.example.com:autre/x.git\n\
             [remote \"origin\"]\n\turl = git@gitlab.example.com:shop/boutique.git\n",
        )
        .unwrap();
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
        assert_eq!(main.gitlab_project.as_deref(), Some("shop/boutique"));
        assert_eq!(linked.gitlab_project.as_deref(), Some("shop/boutique"));

        let gone = resolver.resolve(&base.join("disparu").to_string_lossy());
        assert!(!gone.is_repository);

        // Worktree supprimé : relu sur le disque une fois oublié, il n'est plus un worktree.
        std::fs::remove_dir_all(&wt).unwrap();
        assert!(resolver.resolve(&wt.to_string_lossy()).worktree.is_some(), "encore en cache");
        resolver.forget(&wt.to_string_lossy());
        assert_eq!(resolver.resolve(&wt.to_string_lossy()).worktree, None);
        std::fs::remove_dir_all(base).unwrap();
    }
}
