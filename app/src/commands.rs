//! Commandes IPC. Chaque entrée venant du front est validée ici avant d'atteindre le domaine.

use std::path::PathBuf;

use robiade_core::agents::AgentId;
use robiade_core::forge::MrRef;
use robiade_core::history::{SessionSearch, TranscriptReader};
use robiade_core::stats;
use robiade_core::tasks::{Task, TaskId, TaskRepository};
use tauri::{Manager, State};

use crate::dto::{
    AgentDto, AssetContentDto, EntryDto, EnvironmentDto, FileChangeDto, ForgeDto, MergeRequestDto, MrOutcomesDto, MrRefDto,
    OverviewDto, SearchHitDto, SessionDto, TaskDto, TaskStatusDto,
};
use crate::{now, AppState};

type CmdResult<T> = Result<T, String>;

fn agent_id(raw: &str) -> CmdResult<AgentId> {
    AgentId::parse(raw).map_err(|e| e.to_string())
}

/// Un dossier existant, désigné par un chemin absolu et canonique.
fn project_dir(raw: &str) -> CmdResult<PathBuf> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err("chemin de projet non absolu".into());
    }
    let canonical = path.canonicalize().map_err(|_| "dossier de projet introuvable".to_owned())?;
    canonical.is_dir().then_some(canonical).ok_or_else(|| "le chemin n'est pas un dossier".into())
}

#[tauri::command]
pub fn environment(app: tauri::AppHandle, state: State<AppState>) -> EnvironmentDto {
    EnvironmentDto {
        claude_path: state.agents.claude_path().map(str::to_owned),
        home: app.path().home_dir().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

#[tauri::command]
pub async fn list_history(state: State<'_, AppState>) -> CmdResult<Vec<SessionDto>> {
    let store = state.transcripts.clone();
    let summaries = tauri::async_runtime::spawn_blocking(move || store.summaries())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let names = state.agents.names_by_session();
    Ok(summaries
        .iter()
        .map(|s| {
            let mut dto = SessionDto::new(s, &state.workspaces.resolve(&s.project_path));
            if let Some(name) = names.get(&s.session_id) {
                dto.title = name.clone();
            }
            dto
        })
        .collect())
}

#[tauri::command]
pub async fn read_transcript(state: State<'_, AppState>, session_id: String) -> CmdResult<Vec<EntryDto>> {
    if session_id.is_empty() || !session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("identifiant de session invalide".into());
    }
    let store = state.transcripts.clone();
    let entries = tauri::async_runtime::spawn_blocking(move || store.entries(&session_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(entries.into_iter().map(EntryDto::from).collect())
}

/// Recherche plein texte. L'index est d'abord remis à jour : seuls les transcripts modifiés sont relus.
#[tauri::command]
pub async fn search_history(state: State<'_, AppState>, query: String) -> CmdResult<Vec<SearchHitDto>> {
    let query: String = query.chars().take(200).collect();
    let (store, index) = (state.transcripts.clone(), state.search.clone());
    let hits = tauri::async_runtime::spawn_blocking(move || {
        index.sync(&store.transcript_files())?;
        index.search(&query, 300)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(hits.into_iter().map(SearchHitDto::from).collect())
}

#[tauri::command]
pub async fn stats(
    state: State<'_, AppState>,
    window_days: i64,
    utc_offset_minutes: i64,
    project_root: Option<String>,
) -> CmdResult<OverviewDto> {
    let window_days = window_days.clamp(1, 366);
    let offset_ms = utc_offset_minutes.clamp(-14 * 60, 14 * 60) * 60_000;
    let store = state.transcripts.clone();
    let mut summaries = tauri::async_runtime::spawn_blocking(move || store.summaries())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let project_of = |path: &str| state.workspaces.resolve(path).project_root;
    let in_scope = |path: &str| project_root.as_deref().is_none_or(|root| project_of(path) == root);
    summaries.retain(|s| in_scope(&s.project_path));
    let at = now();
    let mut agents = state.agents.activity_since(at - window_days * 86_400_000);
    agents.retain(|a| in_scope(a.project_path()));
    Ok(OverviewDto::new(
        stats::overview(&summaries, project_of, at, offset_ms, window_days),
        stats::agents_overview(&agents, at - window_days * 86_400_000, at),
    ))
}

#[tauri::command]
pub fn list_tasks(state: State<AppState>) -> CmdResult<Vec<TaskDto>> {
    Ok(state.tasks.all().map_err(|e| e.to_string())?.iter().map(TaskDto::from).collect())
}

#[tauri::command]
pub fn create_task(state: State<AppState>, title: String, project_path: Option<String>) -> CmdResult<TaskDto> {
    let id = TaskId(uuid::Uuid::new_v4().to_string());
    let task = Task::create(id, &title, project_path, now()).map_err(|e| e.to_string())?;
    state.tasks.save(&task).map_err(|e| e.to_string())?;
    Ok((&task).into())
}

#[tauri::command]
pub fn update_task(
    state: State<AppState>,
    id: String,
    title: Option<String>,
    notes: Option<String>,
    status: Option<TaskStatusDto>,
) -> CmdResult<TaskDto> {
    let mut task = state.tasks.get(&TaskId(id)).map_err(|e| e.to_string())?;
    if let Some(title) = title {
        task.rename(&title).map_err(|e| e.to_string())?;
    }
    if let Some(notes) = notes {
        task.set_notes(&notes);
    }
    if let Some(status) = status {
        task.move_to(status.into(), now());
    }
    state.tasks.save(&task).map_err(|e| e.to_string())?;
    Ok((&task).into())
}

#[tauri::command]
pub fn delete_task(state: State<AppState>, id: String) -> CmdResult<()> {
    state.tasks.delete(&TaskId(id)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_agents(state: State<AppState>) -> Vec<AgentDto> {
    state.agents.list()
}

#[tauri::command]
pub fn spawn_agent(
    state: State<AppState>,
    project_path: String,
    resume_session_id: Option<String>,
    cols: u16,
    rows: u16,
) -> CmdResult<AgentDto> {
    let cwd = project_dir(&project_path)?;
    let resume = resume_session_id.as_deref().map(agent_id).transpose()?;
    state.agents.spawn(&cwd, resume, cols.clamp(20, 500), rows.clamp(5, 200))
}

#[tauri::command]
pub fn write_agent(state: State<AppState>, id: String, data: String) -> CmdResult<()> {
    state.agents.process(&agent_id(&id)?)?.write(data.as_bytes())
}

#[tauri::command]
pub fn resize_agent(state: State<AppState>, id: String, cols: u16, rows: u16) -> CmdResult<()> {
    state.agents.process(&agent_id(&id)?)?.resize(cols.clamp(20, 500), rows.clamp(5, 200))
}

#[tauri::command]
pub fn kill_agent(state: State<AppState>, id: String) -> CmdResult<()> {
    state.agents.process(&agent_id(&id)?)?.kill();
    Ok(())
}

#[tauri::command]
pub fn rename_agent(state: State<AppState>, id: String, name: String) -> CmdResult<()> {
    state.agents.rename(&agent_id(&id)?, &name)
}

#[tauri::command]
pub fn forget_agent(state: State<AppState>, id: String) -> CmdResult<()> {
    state.agents.forget(&agent_id(&id)?)
}

#[tauri::command]
pub fn mark_agent_seen(state: State<AppState>, id: String) -> CmdResult<()> {
    state.agents.mark_seen(&agent_id(&id)?);
    Ok(())
}

/// Taille au-delà de laquelle un asset n'est pas chargé dans l'interface.
const MAX_ASSET_BYTES: u64 = 20 * 1024 * 1024;

#[tauri::command]
pub async fn read_asset(state: State<'_, AppState>, id: String, path: String) -> CmdResult<AssetContentDto> {
    let asset = state.agents.asset(&agent_id(&id)?, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let missing = |_| "fichier introuvable : déplacé ou supprimé ?".to_owned();
        let size = std::fs::metadata(&asset.path).map_err(missing)?.len();
        if size > MAX_ASSET_BYTES {
            return Err(format!("fichier trop lourd pour l'aperçu ({} Mo)", size / (1024 * 1024)));
        }
        let bytes = std::fs::read(&asset.path).map_err(|e| e.to_string())?;
        Ok(AssetContentDto::new(&asset, bytes))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Modifications en cours dans le dossier de l'agent.
#[tauri::command]
pub async fn working_changes(state: State<'_, AppState>, id: String) -> CmdResult<Vec<FileChangeDto>> {
    let dir = state.agents.project_path(&agent_id(&id)?)?;
    let vcs = state.vcs.clone();
    let changes = tauri::async_runtime::spawn_blocking(move || vcs.changes(&dir))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(changes.into_iter().map(FileChangeDto::from).collect())
}

/// Diff d'un fichier modifié ; un chemin hors des modifications en cours est refusé par l'adapter.
#[tauri::command]
pub async fn working_diff(state: State<'_, AppState>, id: String, path: String) -> CmdResult<String> {
    let dir = state.agents.project_path(&agent_id(&id)?)?;
    let vcs = state.vcs.clone();
    tauri::async_runtime::spawn_blocking(move || vcs.diff(&dir, &path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Mes MR ouvertes et celles qui attendent ma review, lues ensemble sur GitLab.
#[tauri::command]
pub async fn merge_requests(state: State<'_, AppState>) -> CmdResult<ForgeDto> {
    let forge = state.forge.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (mine, reviews) = std::thread::scope(|s| {
            let reviews = s.spawn(|| forge.review_requests());
            (forge.my_merge_requests(), reviews.join().unwrap_or_else(|_| Ok(vec![])))
        });
        let dto = |mrs: Vec<_>| mrs.into_iter().map(MergeRequestDto::from).collect();
        Ok(ForgeDto { mine: dto(mine?), reviews: dto(reviews?) })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e: robiade_core::forge::ForgeError| e.to_string())
}

/// Au-delà, la page de stats ne relit pas les MR une à une.
const MAX_OUTCOME_LOOKUPS: usize = 300;

/// Part des MR produites par des sessions qui ont été mergées.
#[tauri::command]
pub async fn merge_request_outcomes(state: State<'_, AppState>, refs: Vec<MrRefDto>) -> CmdResult<MrOutcomesDto> {
    let refs: Vec<_> = refs.iter().take(MAX_OUTCOME_LOOKUPS).filter_map(MrRefDto::validate).collect();
    let states = state.mr_states.clone();
    tauri::async_runtime::spawn_blocking(move || states.outcomes(&refs, now()))
        .await
        .map_err(|e| e.to_string())?
        .map(MrOutcomesDto::from)
        .map_err(|e| e.to_string())
}

/// État des MR désignées par leur URL (`pr-link` des sessions), pour colorer leurs badges.
/// Les URL hors de l'instance configurée et les MR illisibles sont absentes de la réponse.
#[tauri::command]
pub async fn merge_request_states(
    state: State<'_, AppState>,
    urls: Vec<String>,
) -> CmdResult<std::collections::HashMap<String, &'static str>> {
    let known: Vec<(String, MrRef)> = urls
        .into_iter()
        .take(MAX_OUTCOME_LOOKUPS)
        .filter_map(|url| MrRef::from_url(&url).map(|r| (url, r)))
        .collect();
    let refs: Vec<MrRef> = known.iter().map(|(_, r)| r.clone()).collect();
    let states = state.mr_states.clone();
    let found = tauri::async_runtime::spawn_blocking(move || states.states(&refs, now()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(known.into_iter().zip(found).filter_map(|((url, _), s)| Some((url, crate::dto::mr_state(s?)))).collect())
}

/// Fermeture confirmée : les agents sont arrêtés, leurs sessions restent reprenables.
#[tauri::command]
pub fn quit(app: tauri::AppHandle, state: State<AppState>) {
    state.quit_confirmed.store(true, std::sync::atomic::Ordering::SeqCst);
    state.agents.kill_all();
    app.exit(0);
}
