//! Commandes IPC. Chaque entrée venant du front est validée ici avant d'atteindre le domaine.

use std::path::{Path, PathBuf};

use robiade_core::agents::AgentId;
use robiade_core::forge::{IssueRef, MrRef};
use robiade_core::issue_start;
use robiade_core::history::{SessionSearch, TranscriptReader};
use robiade_core::stats;
use robiade_core::tasks::{Task, TaskId, TaskRepository};
use tauri::{Manager, State};

use crate::dto::{
    ActionPreviewDto, AgentDto, AssetContentDto, AuditEntryDto, EntryDto, EnvironmentDto, FileChangeDto, ForgeDto,
    IssueDto, IssueRefDto, IssueStartDto, IssueStartedDto, MergeRequestDto, MrActionDto, MrOutcomesDto, MrRefDto,
    OverviewDto, SearchHitDto, SessionDto, TaskDto, TaskStatusDto, WorktreeRemovalDto,
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
    state.agents.spawn(&cwd, resume, None, cols.clamp(20, 500), rows.clamp(5, 200))
}

/// Redémarre un agent sur sa session ; en tâche de fond, l'arrêt du process peut prendre un instant.
#[tauri::command]
pub async fn restart_agent(app: tauri::AppHandle, id: String, cols: u16, rows: u16) -> CmdResult<AgentDto> {
    let id = agent_id(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().agents.restart(&id, cols.clamp(20, 500), rows.clamp(5, 200))
    })
    .await
    .map_err(|e| e.to_string())?
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

/// Issues ouvertes d'un projet GitLab, ou celles qui me sont assignées si `project` est absent.
#[tauri::command]
pub async fn issues(state: State<'_, AppState>, project: Option<String>) -> CmdResult<Vec<IssueDto>> {
    let forge = state.forge.clone();
    let issues = tauri::async_runtime::spawn_blocking(move || match project {
        Some(p) => forge.project_issues(&p),
        None => forge.my_issues(),
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(issues.into_iter().map(IssueDto::from).collect())
}

/// Relit la MR sur GitLab et applique les règles du domaine : rien ne part sur un état périmé.
fn planned(
    state: &AppState,
    target: &MrRefDto,
    action: MrActionDto,
) -> CmdResult<(robiade_core::forge::MergeRequest, robiade_core::merge_actions::Planned)> {
    let mr = target.validate().ok_or("MR invalide")?;
    let current = state.forge.merge_request(&mr).map_err(|e| e.to_string())?;
    let me = state.forge.me().map_err(|e| e.to_string())?;
    let planned = robiade_core::merge_actions::plan(&current, &action.into(), &me).map_err(|e| e.to_string())?;
    Ok((current, planned))
}

/// Ce qu'une action ferait, pour la confirmation. N'écrit rien.
#[tauri::command]
pub async fn preview_mr_action(
    app: tauri::AppHandle,
    target: MrRefDto,
    action: MrActionDto,
) -> CmdResult<ActionPreviewDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (mr, planned) = planned(&state, &target, action)?;
        Ok(ActionPreviewDto {
            project: mr.mr.project,
            iid: mr.mr.iid,
            title: mr.title,
            source_branch: mr.source_branch,
            target_branch: mr.target_branch,
            author: mr.author,
            action: planned.describe(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Action confirmée : replanifiée sur l'état courant, envoyée, puis inscrite au journal d'audit.
#[tauri::command]
pub async fn run_mr_action(app: tauri::AppHandle, target: MrRefDto, action: MrActionDto) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (mr, planned) = planned(&state, &target, action)?;
        let outcome = state.forge.execute(&mr.mr, &planned).map_err(|e| e.to_string());
        let entry = robiade_core::merge_actions::AuditEntry::new(now(), &mr.mr, &planned, outcome.clone());
        if let Err(e) = state.audit.record(&entry) {
            eprintln!("{e}");
        }
        outcome.map(|()| planned.describe())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn audit_log(state: State<AppState>) -> CmdResult<Vec<AuditEntryDto>> {
    Ok(state.audit.recent(50).map_err(|e| e.to_string())?.into_iter().map(AuditEntryDto::from).collect())
}

/// Dépôt principal désigné par l'interface, rattaché au projet GitLab de l'issue : rien d'autre.
fn issue_repository(state: &AppState, project_root: &str, issue: &IssueRef) -> CmdResult<String> {
    let dir = project_dir(project_root)?.to_string_lossy().into_owned();
    let workspace = state.workspaces.resolve(&dir);
    let same_project = workspace.gitlab_project.as_deref().is_some_and(|p| p.eq_ignore_ascii_case(&issue.project));
    if workspace.project_root != dir || workspace.worktree.is_some() || !same_project {
        return Err("ce dossier n'est pas le dépôt du projet de l'issue".into());
    }
    Ok(dir)
}

/// Ce que « démarrer un agent sur cette issue » proposera : branche, bases, prompt à relire.
#[tauri::command]
pub async fn prepare_issue_start(
    app: tauri::AppHandle,
    project_root: String,
    issue: IssueRefDto,
) -> CmdResult<IssueStartDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let issue_ref = issue.validate().ok_or("issue invalide")?;
        let repository = issue_repository(&state, &project_root, &issue_ref)?;
        let found = state.forge.issue(&issue_ref).map_err(|e| e.to_string())?;
        let bases = state.worktrees.bases(&repository).map_err(|e| e.to_string())?;
        if bases.is_empty() {
            return Err("aucune base origin/staging, origin/main ni origin/master dans ce dépôt".into());
        }
        let branch = issue_start::branch_name(&found);
        Ok(IssueStartDto {
            repository: repository.clone(),
            worktree_dir: issue_start::worktree_dir(&repository, &branch),
            prompt: issue_start::initial_prompt(&found, &branch),
            tab_name: issue_start::tab_name(&found),
            branch,
            bases,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Crée la branche et le worktree, puis lance l'agent avec le prompt relu.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn start_issue_agent(
    app: tauri::AppHandle,
    project_root: String,
    issue: IssueRefDto,
    branch: String,
    base: String,
    prompt: String,
    cols: u16,
    rows: u16,
) -> CmdResult<IssueStartedDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let issue_ref = issue.validate().ok_or("issue invalide")?;
        let repository = issue_repository(&state, &project_root, &issue_ref)?;
        if !issue_start::is_valid_branch(&branch) {
            return Err("nom de branche refusé".into());
        }
        let prompt = prompt.trim().to_owned();
        if prompt.is_empty() || prompt.chars().count() > issue_start::PROMPT_MAX_CHARS {
            return Err("prompt vide ou trop long".into());
        }
        let found = state.forge.issue(&issue_ref).map_err(|e| e.to_string())?;
        let dir = issue_start::worktree_dir(&repository, &branch);
        let fetched = state.worktrees.create(&repository, &branch, &base, &dir).map_err(|e| e.to_string())?;
        let agent =
            state.agents.spawn(Path::new(&dir), None, Some(prompt), cols.clamp(20, 500), rows.clamp(5, 200))?;
        state.agents.rename(&agent_id(&agent.id)?, &issue_start::tab_name(&found))?;
        Ok(IssueStartedDto { agent_id: agent.id, fetched })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Racine d'un worktree lié, désignée par l'interface, et le dépôt principal auquel il appartient.
fn linked_worktree(state: &AppState, raw: &str) -> CmdResult<(String, String)> {
    let dir = project_dir(raw)?.to_string_lossy().into_owned();
    let workspace = state.workspaces.resolve(&dir);
    if workspace.worktree.as_deref() != Some(dir.as_str()) || !workspace.is_repository {
        return Err("ce dossier n'est pas un worktree lié à un dépôt".into());
    }
    Ok((dir, workspace.project_root))
}

/// Ce qui empêche de supprimer le worktree : un agent dedans, ou des modifications non commitées.
fn removal_blocker(state: &AppState, dir: &str) -> CmdResult<Option<String>> {
    if state.agents.running_in(dir) {
        return Ok(Some("un agent tourne encore dans ce worktree : arrête-le d'abord".into()));
    }
    let changes = state.vcs.changes(dir).map_err(|e| e.to_string())?;
    Ok((!changes.is_empty()).then(|| {
        format!("{} fichier(s) modifié(s) ou non suivi(s) : commite-les ou annule-les d'abord", changes.len())
    }))
}

/// Ce que « supprimer le worktree » ferait : cible exacte, branche, et raison d'un refus.
#[tauri::command]
pub async fn prepare_worktree_removal(app: tauri::AppHandle, path: String) -> CmdResult<WorktreeRemovalDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (dir, repository) = linked_worktree(&state, &path)?;
        Ok(WorktreeRemovalDto {
            branch: state.worktrees.branch(&dir).map_err(|e| e.to_string())?,
            blocker: removal_blocker(&state, &dir)?,
            path: dir,
            repository,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Supprime le worktree, sans force ; sa branche reste. Les vérifications sont refaites ici.
#[tauri::command]
pub async fn remove_worktree(app: tauri::AppHandle, path: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (dir, repository) = linked_worktree(&state, &path)?;
        if let Some(blocker) = removal_blocker(&state, &dir)? {
            return Err(blocker);
        }
        state.worktrees.remove(&repository, &dir).map_err(|e| e.to_string())?;
        state.workspaces.forget(&dir);
        state.agents.reattach(&dir);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
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
