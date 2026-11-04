//! Branches de tronc : réglées dépôt par dépôt (Réglages, section Projets), sinon la branche par
//! défaut du remote. Elles donnent la base des worktrees et les wagons des trains de MR.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use robiade_adapters::sqlite_trunks::SqliteTrunks;
use robiade_adapters::workspace::WorkspaceResolver;
use robiade_core::forge::{Forge, ForgeError};
use robiade_core::history::TranscriptReader;
use robiade_core::tasks::TaskRepository;
use robiade_core::trunks::TrunkBranches;
use serde::Serialize;
use tauri::Manager;

use crate::AppState;

type CmdResult<T> = Result<T, String>;

pub struct Trunks {
    store: SqliteTrunks,
    workspaces: Arc<WorkspaceResolver>,
    forge: Arc<dyn Forge>,
    /// Branche par défaut des projets de forge sans réglage, lue une fois par lancement.
    defaults: Mutex<HashMap<String, String>>,
}

impl Trunks {
    pub fn new(store: SqliteTrunks, workspaces: Arc<WorkspaceResolver>, forge: Arc<dyn Forge>) -> Self {
        Self { store, workspaces, forge, defaults: Mutex::default() }
    }

    /// Troncs réglés d'un dépôt principal ; `None` : la branche par défaut du remote (`origin/HEAD`).
    pub fn of_repository(&self, repository: &str) -> CmdResult<Option<TrunkBranches>> {
        self.store.get(repository).map_err(|e| e.to_string())
    }

    pub fn set(&self, repository: &str, trunks: Option<&TrunkBranches>) -> CmdResult<()> {
        self.store.set(repository, trunks).map_err(|e| e.to_string())
    }

    /// Troncs d'un projet de forge : ceux du dépôt local qui le porte, s'il est réglé ; sinon la
    /// branche par défaut du projet, lue sur la forge. Une MR vit sur le remote : son `origin/HEAD`
    /// local peut manquer ou dater, la forge non.
    pub fn of_project(&self, project: &str) -> Result<TrunkBranches, ForgeError> {
        let configured = self.store.all().map_err(|e| ForgeError::Api(e.to_string()))?;
        let local = configured.into_iter().find(|(repository, _)| {
            let found = self.workspaces.resolve(repository).gitlab_project;
            found.is_some_and(|p| p.eq_ignore_ascii_case(project))
        });
        if let Some((_, trunks)) = local {
            return Ok(trunks);
        }
        let key = project.to_ascii_lowercase();
        let known = self.defaults.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned();
        let branch = match known {
            Some(branch) => branch,
            None => {
                let branch = self.forge.default_branch(project)?;
                self.defaults.lock().unwrap_or_else(|e| e.into_inner()).insert(key, branch.clone());
                branch
            }
        };
        TrunkBranches::new([branch]).map_err(|e| ForgeError::Api(e.to_string()))
    }

    /// Troncs de chaque projet cité, lus en parallèle ; un projet illisible est absent.
    pub fn of_projects<'a>(&self, projects: impl IntoIterator<Item = &'a str>) -> HashMap<String, TrunkBranches> {
        let mut distinct: Vec<&str> = projects.into_iter().collect();
        distinct.sort_unstable();
        distinct.dedup();
        std::thread::scope(|s| {
            let reads: Vec<_> = distinct.iter().map(|p| (*p, s.spawn(|| self.of_project(p)))).collect();
            reads
                .into_iter()
                .filter_map(|(p, h)| Some((p.to_owned(), h.join().ok()?.ok()?)))
                .collect()
        })
    }
}

/// Dépôts principaux déjà connus : ceux de l'historique des sessions et des tâches. Lu une seule
/// fois, pour donner aux dépôts d'un profil antérieur les troncs que le code fixait.
pub fn known_repositories(
    transcripts: &impl TranscriptReader,
    tasks: &impl TaskRepository,
    workspaces: &WorkspaceResolver,
) -> Vec<String> {
    let sessions = transcripts.summaries().unwrap_or_default().into_iter().map(|s| s.project_path);
    let tasks = tasks.all().unwrap_or_default().into_iter().filter_map(|t| t.project_path().map(str::to_owned));
    let mut found: Vec<String> = sessions
        .chain(tasks)
        .map(|path| workspaces.resolve(&path))
        .filter(|w| w.is_repository)
        .map(|w| w.project_root)
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

/// Message quand aucune base n'existe pour un nouveau worktree.
pub fn no_base(trunks: Option<&TrunkBranches>) -> String {
    match trunks {
        Some(t) => format!(
            "aucune branche de tronc de ce dépôt n'existe sur origin ({}) : règle-les dans Réglages",
            t.as_slice().join(", ")
        ),
        None => "origin/HEAD absent : lance `git remote set-head origin --auto` dans ce dépôt, ou règle ses \
                 branches de tronc dans Réglages"
            .into(),
    }
}

/// Bases proposées pour un worktree du dépôt, et refus s'il n'y en a aucune.
pub fn bases(state: &AppState, repository: &str) -> CmdResult<Vec<String>> {
    let trunks = state.trunks.of_repository(repository)?;
    let bases = state.worktrees.bases(repository, trunks.as_ref()).map_err(|e| e.to_string())?;
    if bases.is_empty() {
        return Err(no_base(trunks.as_ref()));
    }
    Ok(bases)
}

/// Une base choisie à l'écran n'est acceptée que si elle fait toujours partie des bases du dépôt.
pub fn check_base(state: &AppState, repository: &str, base: &str) -> CmdResult<()> {
    bases(state, repository)?.iter().any(|b| b == base).then_some(()).ok_or_else(|| format!("base refusée : {base}"))
}

/// Réglage d'un dépôt pour la page Réglages.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryTrunksDto {
    pub repository: String,
    /// `None` : pas de réglage.
    pub configured: Option<Vec<String>>,
    /// Branche par défaut du remote (`origin/HEAD`), qui sert sans réglage ; `None` si absente.
    pub remote_default: Option<String>,
}

fn describe(state: &AppState, repository: String) -> CmdResult<RepositoryTrunksDto> {
    let configured = state.trunks.of_repository(&repository)?.map(|t| t.as_slice().to_vec());
    let remote = state.worktrees.bases(&repository, None).map_err(|e| e.to_string())?;
    let remote_default = remote.first().map(|b| b.trim_start_matches("origin/").to_owned());
    Ok(RepositoryTrunksDto { repository, configured, remote_default })
}

#[tauri::command]
pub async fn repository_trunks(app: tauri::AppHandle, repository: String) -> CmdResult<RepositoryTrunksDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let repository = crate::commands::main_repository(&state, &repository)?;
        describe(&state, repository)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Règle les troncs d'un dépôt, dans l'ordre ; une liste vide retire le réglage.
#[tauri::command]
pub async fn save_repository_trunks(
    app: tauri::AppHandle,
    repository: String,
    branches: Vec<String>,
) -> CmdResult<RepositoryTrunksDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let repository = crate::commands::main_repository(&state, &repository)?;
        let trunks = if branches.iter().all(|b| b.trim().is_empty()) {
            None
        } else {
            Some(TrunkBranches::new(&branches).map_err(|e| e.to_string())?)
        };
        state.trunks.set(&repository, trunks.as_ref())?;
        describe(&state, repository)
    })
    .await
    .map_err(|e| e.to_string())?
}
