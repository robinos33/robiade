//! Noyau partagé : rattacher un dossier de travail à son projet.

const CLAUDE_WORKTREES: &str = "/.claude/worktrees/";

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
}
