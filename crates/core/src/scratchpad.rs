//! Scratchpad d'un agent : le dossier temporaire que Claude Code donne à chaque session, où
//! l'agent range ce qu'il veut me montrer (scripts, requêtes, brouillons).
//!
//! Claude Code 2.1 le place en `<racine>/<dossier de lancement>/<session>/scratchpad`, où la racine
//! est `/tmp/claude-<uid>` résolu (`/private/tmp/…` sur macOS). Le dossier de lancement y est
//! réduit à un nom (tout ce qui n'est ni lettre ni chiffre devient `-`, haché au-delà de 200
//! caractères) : il n'est pas recalculé ici, car la session, un UUID, suffit à désigner l'agent.

use std::path::{Component, Path};

/// `path`, résolu (sans `..` ni lien), est dans le scratchpad de l'une des `sessions`, sans être
/// le dossier lui-même. `root` est la racine résolue `/tmp/claude-<uid>`.
pub fn holds(root: &Path, sessions: &[String], path: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(root) else { return false };
    let parts: Option<Vec<&str>> = rest
        .components()
        .map(|c| match c {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect();
    match parts.as_deref() {
        Some([_project, session, "scratchpad", _, ..]) => sessions.iter().any(|s| s == session),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/private/tmp/claude-501";
    const SESSION: &str = "bd619fb3-b93e-405a-a6e6-70e17e566137";

    fn holds_path(path: &str) -> bool {
        holds(Path::new(ROOT), &[SESSION.to_owned()], Path::new(path))
    }

    #[test]
    fn holds_any_file_under_the_scratchpad_of_a_session_of_the_agent() {
        let dir = format!("{ROOT}/-Users-me-projet/{SESSION}/scratchpad");
        assert!(holds_path(&format!("{dir}/bascule.groovy")));
        assert!(holds_path(&format!("{dir}/sous/dossier/.env")));
        // Le nom du dossier de lancement n'est pas recalculé : il peut être haché.
        assert!(holds_path(&format!("{ROOT}/-Users-me-tres-long-a1b2c3/{SESSION}/scratchpad/a.sql")));
    }

    #[test]
    fn refuses_anything_else() {
        let other = "9d259b93-5dc0-47c3-b1c8-3084e032dd44";
        for path in [
            format!("{ROOT}/-Users-me-projet/{SESSION}/scratchpad"),
            format!("{ROOT}/-Users-me-projet/{other}/scratchpad/a.sql"),
            format!("{ROOT}/-Users-me-projet/{SESSION}/tasks/a.output"),
            format!("{ROOT}/-Users-me-projet/{SESSION}/scratchpad/../../x.sql"),
            format!("{ROOT}/{SESSION}/scratchpad/a.sql"),
            format!("/tmp/claude-501/-Users-me-projet/{SESSION}/scratchpad/a.sql"),
            format!("/private/tmp/claude-502/-Users-me-projet/{SESSION}/scratchpad/a.sql"),
            "/private/tmp/autre.txt".to_owned(),
        ] {
            assert!(!holds_path(&path), "{path}");
        }
    }
}
