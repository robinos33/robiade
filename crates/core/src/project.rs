//! Noyau partagé : rattacher un dossier de travail à son projet.

const CLAUDE_WORKTREES: &str = "/.claude/worktrees/";
const GIT_WORKTREES: &str = "/.git/worktrees/";

/// Où vit un dossier de travail : le dépôt principal auquel il appartient, et le worktree
/// s'il n'est pas dans l'arbre principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// Racine de l'arbre de travail principal du dépôt (ou le dossier lui-même, hors Git).
    pub project_root: String,
    /// Racine du worktree lié, s'il y en a un et qu'il existe encore.
    pub worktree: Option<String>,
    /// `project_root` est un dépôt Git présent sur le disque : un vrai projet, pas un dossier
    /// de passage ni un worktree supprimé dont on ne retrouve plus le dépôt.
    pub is_repository: bool,
    /// Projet de forge du remote `origin`, s'il est sur l'instance réglée.
    pub gitlab_project: Option<String>,
}

impl Workspace {
    /// Dossier hors Git, ou disparu : seule la convention de Claude Code permet encore de
    /// retrouver le dépôt.
    pub fn outside_git(path: &str) -> Self {
        Self { project_root: root_of(path).to_owned(), worktree: None, is_repository: false, gitlab_project: None }
    }

    pub fn project_name(&self) -> &str {
        display_name(&self.project_root)
    }

    pub fn worktree_name(&self) -> Option<&str> {
        self.worktree.as_deref().map(display_name)
    }
}

/// `gitdir` d'un worktree lié (`<dépôt>/.git/worktrees/<nom>`) → racine du dépôt principal.
pub fn repository_of_gitdir(gitdir: &str) -> Option<&str> {
    gitdir.trim_end().rfind(GIT_WORKTREES).map(|i| &gitdir[..i])
}

/// Racine du projet d'un dossier de travail. Un worktree créé par Claude Code
/// (`<repo>/.claude/worktrees/<nom>`) appartient au dépôt qui le contient.
pub fn root_of(path: &str) -> &str {
    path.find(CLAUDE_WORKTREES).map_or(path, |i| &path[..i])
}

/// Nom court affiché : dernier segment du chemin racine.
pub fn display_name(path: &str) -> &str {
    root_of(path).trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claude_worktree_belongs_to_its_repository() {
        let wt = "/Users/r/PhpstormProjects/boutique/.claude/worktrees/ticket-4626";
        assert_eq!(root_of(wt), "/Users/r/PhpstormProjects/boutique");
        assert_eq!(display_name(wt), "boutique");
        assert_eq!(root_of("/Users/r/ade"), "/Users/r/ade");
    }

    #[test]
    fn a_linked_worktree_points_to_its_repository() {
        let gitdir = "/Users/r/PhpstormProjects/marque_blanche/.git/worktrees/mb-wt-fonts\n";
        assert_eq!(repository_of_gitdir(gitdir), Some("/Users/r/PhpstormProjects/marque_blanche"));
        assert_eq!(repository_of_gitdir("/Users/r/x/.git/modules/vendor"), None);
    }
}
