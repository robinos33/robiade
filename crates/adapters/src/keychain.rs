//! Secrets lus dans le trousseau macOS. Robiade n'en écrit aucun : un secret se saisit dans un
//! vrai terminal (`security add-generic-password -s <service> -a "$USER" -w`).

use std::process::{Command, Stdio};

use secrecy::SecretString;

/// Mot de passe générique du service donné, ou `None` s'il est absent ou illisible.
/// `/usr/bin/security` en chemin absolu : un `security` placé plus tôt dans le PATH ne voit rien.
pub fn generic_password(service: &str) -> Option<SecretString> {
    let out = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-w"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // Retaillé sur place, sans copie : seule la `SecretString` détient le token, effacé au drop.
    let mut value = String::from_utf8(out.stdout).ok()?;
    while value.ends_with('\n') {
        value.pop();
    }
    (!value.is_empty()).then(|| SecretString::from(value))
}
