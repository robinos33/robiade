//! Environnement du shell de login : une app lancée depuis le Finder hérite d'un PATH minimal.

use std::process::Command;

const MARK: &str = "@@ROBIADE@@";

pub struct LoginEnv {
    pub path: String,
    pub claude: Option<String>,
}

/// Interroge `$SHELL -lc` une seule fois. Aucune valeur extérieure n'est interpolée dans la commande.
pub fn resolve() -> LoginEnv {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let script = format!(r#"printf '{MARK}%s{MARK}%s{MARK}' "$PATH" "$(command -v claude)""#);
    let output = Command::new(shell).args(["-lc", &script]).output();
    let fallback = || LoginEnv { path: std::env::var("PATH").unwrap_or_default(), claude: None };
    let Ok(output) = output else { return fallback() };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parts = stdout.split(MARK).skip(1);
    match (parts.next(), parts.next()) {
        (Some(path), Some(claude)) => LoginEnv {
            path: path.to_owned(),
            claude: (!claude.is_empty()).then(|| claude.to_owned()),
        },
        _ => fallback(),
    }
}
