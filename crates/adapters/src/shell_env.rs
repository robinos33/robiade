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

/// Au-delà, le shell interactif est abandonné : un `.zshrc` qui attend quelque chose ne bloque pas
/// le démarrage.
const INTERACTIVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// `VAULT_ADDR` tel que mon terminal le voit : on l'exporte d'ordinaire dans `.zshrc`, que seul un
/// shell interactif lit. Lancé une seule fois, pour reprendre les réglages d'un ancien profil.
pub fn interactive_vault_addr() -> Option<String> {
    use std::io::Read;
    use std::process::Stdio;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let script = format!(r#"printf '{MARK}%s{MARK}' "$VAULT_ADDR""#);
    let mut child = Command::new(shell)
        .args(["-lic", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // Lu jusqu'à la seconde marque, pas jusqu'à la fin du flux : un démon lancé par le `.zshrc`
    // hérite de la sortie et la garde ouverte bien après la fin du shell.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut seen, mut chunk) = (Vec::new(), [0u8; 4096]);
        while let Ok(n @ 1..) = stdout.read(&mut chunk) {
            seen.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&seen);
            if let Some(value) = text.split(MARK).nth(1).filter(|_| text.matches(MARK).count() >= 2) {
                let _ = tx.send(value.trim().to_owned());
                return;
            }
        }
    });
    let value = rx.recv_timeout(INTERACTIVE_TIMEOUT).ok();
    let _ = child.kill();
    let _ = child.wait();
    value.filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    /// Dépend de mon `.zshrc` : `cargo test -- --ignored` sur un poste où `VAULT_ADDR` est exporté.
    #[test]
    #[ignore]
    fn reads_vault_addr_as_my_terminal_sees_it() {
        let started = std::time::Instant::now();
        let found = super::interactive_vault_addr();
        assert!(found.is_some_and(|a| a.starts_with("https://")), "VAULT_ADDR introuvable");
        assert!(started.elapsed() < super::INTERACTIVE_TIMEOUT);
    }
}
