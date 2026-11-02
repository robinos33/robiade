//! Mise à jour au lancement : l'app se compare à `origin/main` et, en retard, se reconstruit
//! en tâche de fond. L'interface lit l'état à son chargement, puis suit `app:update`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use robiade_adapters::self_update::SourceRepo;
use robiade_core::updates::{self, UpdateStatus};
use tauri::Emitter;

use crate::dto::UpdateDto;

/// Commit dont la build est issue, posé par `scripts/install.sh`.
pub const COMMIT: Option<&str> = option_env!("ROBIADE_COMMIT");
/// `.git` commun du dépôt d'où elle a été construite.
const REPO: Option<&str> = option_env!("ROBIADE_REPO");

/// Construite avec des modifications non commitées : son commit ne la décrit qu'en partie.
pub fn dirty() -> bool {
    option_env!("ROBIADE_DIRTY") == Some("1")
}

#[derive(Default)]
pub struct Updates {
    /// `None` tant que rien n'est décidé, ou pour une build qui ne se met pas à jour.
    status: Mutex<Option<UpdateStatus>>,
}

impl Updates {
    pub fn status(&self) -> Option<UpdateStatus> {
        self.status.lock().ok()?.clone()
    }

    /// Lance la vérification en tâche de fond. Un `tauri dev`, ou une build qui ne vient pas
    /// d'`install.sh`, ne se met jamais à jour.
    pub fn start(self: &Arc<Self>, app: tauri::AppHandle, workspace: PathBuf, path: String) {
        let (Some(commit), Some(repo)) = (COMMIT, REPO) else { return };
        if cfg!(debug_assertions) {
            return;
        }
        let this = self.clone();
        std::thread::spawn(move || {
            let source = SourceRepo::new(PathBuf::from(repo), &workspace, path);
            updates::update(&source, commit, &mut |status| {
                if let UpdateStatus::Failed { output } = &status {
                    eprintln!("mise à jour impossible : {output}");
                }
                // Rangé avant d'être annoncé : une interface qui se charge entre les deux le lit.
                let dto = UpdateDto::from(&status);
                if let Ok(mut current) = this.status.lock() {
                    *current = Some(status);
                }
                let _ = app.emit("app:update", dto);
            });
        });
    }
}
