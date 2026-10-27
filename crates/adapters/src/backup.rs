//! Exclusion de Time Machine : un index qui recopie les transcripts n'a rien à faire dans une sauvegarde.

use std::path::Path;
use std::process::Command;

/// Exclusion « collante » posée par `tmutil` : elle suit le dossier et ne demande pas de droits root.
pub fn exclude_from_backups(path: &Path) -> std::io::Result<()> {
    let status = Command::new("/usr/bin/tmutil").arg("addexclusion").arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("tmutil addexclusion a échoué ({status})")))
    }
}
