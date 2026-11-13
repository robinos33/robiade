//! Dossiers que Claude Code a approuvés : ceux où j'ai accepté sa question de confiance, notés
//! `projects.<chemin>.hasTrustDialogAccepted` dans `~/.claude.json`.
//!
//! `claude -p` ne pose pas cette question : lancé dans un dossier, il applique ses réglages de
//! projet (`.claude/settings.json`, dont `apiKeyHelper` et consorts, qui lancent des commandes).
//! Robiade ne le lance donc, pour une routine de skill, que là où Claude Code l'a déjà approuvé.
//!
//! Format sans contrat stable : un fichier absent ou illisible n'approuve rien.

use std::path::Path;

use serde_json::Value;

/// Au-delà, le fichier n'est pas lu : il n'approuve rien.
const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// `dir`, ou un dossier qui le contient, est approuvé dans `claude_json`. Le chemin est d'abord
/// résolu : un `..` ou un lien ne le fait pas passer pour un dossier approuvé.
pub fn is_trusted(claude_json: &Path, home: &Path, dir: &Path) -> bool {
    let (Ok(dir), Some(projects)) = (std::fs::canonicalize(dir), read(claude_json)) else { return false };
    let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    trusted_in(&projects, &home, &dir)
}

/// La racine et le dossier personnel ne comptent que pour eux-mêmes : les approuver n'approuve pas
/// tout le disque. `dir` est déjà résolu.
fn trusted_in(projects: &serde_json::Map<String, Value>, home: &Path, dir: &Path) -> bool {
    let accepted = |p: &Path| {
        projects
            .get(p.to_string_lossy().as_ref())
            .and_then(|v| v.get("hasTrustDialogAccepted"))
            .and_then(Value::as_bool)
            == Some(true)
    };
    if accepted(dir) {
        return true;
    }
    dir.ancestors().skip(1).filter(|a| *a != Path::new("/") && *a != home).any(accepted)
}

fn read(claude_json: &Path) -> Option<serde_json::Map<String, Value>> {
    let meta = std::fs::metadata(claude_json).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let value: Value = serde_json::from_slice(&std::fs::read(claude_json).ok()?).ok()?;
    value.get("projects")?.as_object().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(name: &str, json: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("robiade-trust-{name}-{}.json", std::process::id()));
        std::fs::write(&path, json).unwrap();
        path
    }

    fn projects(json: &str) -> serde_json::Map<String, Value> {
        serde_json::from_str::<Value>(json).unwrap()["projects"].as_object().unwrap().clone()
    }

    #[test]
    fn a_folder_is_trusted_by_itself_or_by_a_parent() {
        let projects = projects(
            r#"{"projects": {
                "/Users/me/code/app": {"hasTrustDialogAccepted": true},
                "/Users/me/code/doute": {"hasTrustDialogAccepted": false},
                "/Users/me/code/vieux": {}
            }}"#,
        );
        let home = Path::new("/Users/me");
        let trusted = |dir: &str| trusted_in(&projects, home, Path::new(dir));
        assert!(trusted("/Users/me/code/app"));
        assert!(trusted("/Users/me/code/app/.claude/worktrees/ticket-1"));
        assert!(!trusted("/Users/me/code/doute"));
        assert!(!trusted("/Users/me/code/vieux"));
        assert!(!trusted("/Users/me/code/autre"));
        assert!(!trusted("/Users/me/code"));
    }

    #[test]
    fn trusting_the_root_or_home_does_not_trust_everything() {
        let projects =
            projects(r#"{"projects": {"/": {"hasTrustDialogAccepted": true}, "/Users/me": {"hasTrustDialogAccepted": true}}}"#);
        let home = Path::new("/Users/me");
        assert!(trusted_in(&projects, home, Path::new("/Users/me")));
        assert!(!trusted_in(&projects, home, Path::new("/Users/me/telechargements/depot")));
        assert!(!trusted_in(&projects, home, Path::new("/tmp/depot")));
    }

    #[test]
    fn the_folder_is_resolved_before_being_compared() {
        let root = std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("robiade-trust-dirs-{}", std::process::id()));
        let (app, other) = (root.join("app"), root.join("autre"));
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let path = config("resolved", &format!(r#"{{"projects": {{"{}": {{"hasTrustDialogAccepted": true}}}}}}"#, app.display()));
        let home = Path::new("/Users/me");
        assert!(is_trusted(&path, home, &app));
        assert!(is_trusted(&path, home, Path::new(&format!("{}/", app.display()))));
        assert!(!is_trusted(&path, home, &app.join("..").join("autre")));
        std::os::unix::fs::symlink(&other, app.join("lien")).unwrap();
        assert!(!is_trusted(&path, home, &app.join("lien")));
        assert!(!is_trusted(&path, home, &app.join("absent")));
    }

    #[test]
    fn a_missing_or_unreadable_file_trusts_nothing() {
        let home = Path::new("/Users/me");
        assert!(!is_trusted(Path::new("/nulle/part.json"), home, Path::new("/Users/me/code/app")));
        let broken = config("broken", "{pas du json");
        assert!(!is_trusted(&broken, home, Path::new("/Users/me/code/app")));
    }
}
