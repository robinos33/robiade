//! Commandes IPC. Chaque entrée venant du front est validée ici avant d'atteindre le domaine.

use std::path::{Path, PathBuf};

use robiade_core::agents::AgentId;
use robiade_adapters::keychain;
use robiade_core::containers::{Container, ContainerAction, ContainerId};
use robiade_core::databases::{Connection, ConnectionId, ConnectionRepository, Part, Source, Target};
use robiade_core::db_browse::{page, page_query, PageRequest, TableRef};
use robiade_core::databases::DbError;
use robiade_core::db_console::{self, Admission, ConsoleHistory, Policy, StatementKind};
use robiade_core::merge_actions::AuditEntry;
use robiade_core::forge::{Issue, IssueRef, MrRef};
use robiade_core::{issue_start, task_start};
use robiade_core::history::{SessionSearch, TranscriptReader};
use robiade_core::contributions::{self, Granularity};
use robiade_core::stats;
use robiade_core::tasks::{DueDate, SessionId, Task, TaskId, TaskRepository};
use robiade_core::assets::AssetKind;
use robiade_core::code::{self, Peek};
use robiade_core::vcs::FileChange;
use secrecy::ExposeSecret;
use tauri::{Manager, State};

use crate::dto::{
    ActionPreviewDto, AgentDto, AssetContentDto, AuditEntryDto, ConsoleHistoryDto, ConsoleReplyDto, ConsoleResultDto, FinishedWriteDto,
    ContainerActionPreviewDto, ContainerDto, ContributionBucketDto, DbConnectionDto, DbConnectionInputDto, DbOverviewDto, DbProbeDto, DbTargetDto, EntryDto, EnvironmentDto,
    FileChangeDto, ForgeDto, GranularityDto, IssueActionDto, IssueActionPreviewDto, IssueDto, IssueRefDto, IssueStartDto,
    IssueStartedDto, MergeRequestDto, MrActionDto, MrOutcomesDto, MrRefDto, NewIssueDto, OverviewDto, PageDto,
    PageRequestDto, ProjectFileDto, ProjectFilesDto, RelationDto, SearchHitDto, SessionDto, TaskDto, TaskStatusDto, WorktreeCreationDto, WorktreeDto,
    UpdateDto, WorktreeRemovalDto,
};
use crate::db_sessions::OpenSession;
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
        commit: crate::updates::COMMIT.map(str::to_owned),
        dirty: crate::updates::dirty(),
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

/// Mes contributions GitLab par période, tous projets confondus : un événement GitLab ne porte
/// que l'identifiant de son projet, pas son chemin.
#[tauri::command]
pub async fn gitlab_activity(
    state: State<'_, AppState>,
    granularity: GranularityDto,
    utc_offset_minutes: i64,
) -> CmdResult<Vec<ContributionBucketDto>> {
    let granularity = Granularity::from(granularity);
    let offset_ms = utc_offset_minutes.clamp(-14 * 60, 14 * 60) * 60_000;
    let at = now();
    let from = contributions::window_start(granularity, at, offset_ms);
    let cache = state.contributions.clone();
    let events = tauri::async_runtime::spawn_blocking(move || cache.since(from, at))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(contributions::by_period(&events, granularity, at, offset_ms).into_iter().map(ContributionBucketDto::from).collect())
}

#[tauri::command]
pub fn list_tasks(state: State<AppState>) -> CmdResult<Vec<TaskDto>> {
    Ok(state.tasks.all().map_err(|e| e.to_string())?.iter().map(TaskDto::from).collect())
}

#[tauri::command]
pub fn create_task(
    state: State<AppState>,
    title: String,
    project_path: Option<String>,
    due_on: Option<String>,
) -> CmdResult<TaskDto> {
    let id = TaskId(uuid::Uuid::new_v4().to_string());
    let mut task = Task::create(id, &title, project_path, now()).map_err(|e| e.to_string())?;
    task.schedule(due_on.as_deref().map(DueDate::parse).transpose().map_err(|e| e.to_string())?);
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

/// Pose ou retire (`None`) l'échéance : une commande à part, `update_task` ne sachant pas distinguer
/// un champ absent d'un champ vidé.
#[tauri::command]
pub fn schedule_task(state: State<AppState>, id: String, due_on: Option<String>) -> CmdResult<TaskDto> {
    let mut task = state.tasks.get(&TaskId(id)).map_err(|e| e.to_string())?;
    task.schedule(due_on.as_deref().map(DueDate::parse).transpose().map_err(|e| e.to_string())?);
    state.tasks.save(&task).map_err(|e| e.to_string())?;
    Ok((&task).into())
}

/// Lie ou délie (`None`) une issue d'équipe. `reference` est saisie à la main : URL, `projet#12`, ou
/// `#12` dans le projet GitLab du dépôt de la tâche.
#[tauri::command]
pub fn link_task_issue(state: State<AppState>, id: String, reference: Option<String>) -> CmdResult<TaskDto> {
    let mut task = state.tasks.get(&TaskId(id)).map_err(|e| e.to_string())?;
    let issue = match reference.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        None => None,
        Some(raw) => {
            let gitlab_project = task.project_path().and_then(|p| state.workspaces.resolve(p).gitlab_project);
            let issue = IssueRef::parse(raw, gitlab_project.as_deref());
            Some(issue.ok_or_else(|| match gitlab_project {
                Some(_) => format!("issue non reconnue : {raw} (attendu #12, groupe/projet#12 ou son URL)"),
                None => format!("issue non reconnue : {raw} (attendu groupe/projet#12 ou son URL)"),
            })?)
        }
    };
    task.link_issue(issue);
    state.tasks.save(&task).map_err(|e| e.to_string())?;
    Ok((&task).into())
}

/// Lie ou délie (`None`) une session Claude Code.
#[tauri::command]
pub fn link_task_session(state: State<AppState>, id: String, session_id: Option<String>) -> CmdResult<TaskDto> {
    let mut task = state.tasks.get(&TaskId(id)).map_err(|e| e.to_string())?;
    task.link_session(session_id.as_deref().map(SessionId::parse).transpose().map_err(|e| e.to_string())?);
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

/// Racine que l'onglet Code peut lire : celle d'un dépôt Git ou d'un de ses worktrees liés.
fn code_root(state: &AppState, raw: &str) -> CmdResult<PathBuf> {
    let dir = project_dir(raw)?;
    let path = dir.to_string_lossy();
    let workspace = state.workspaces.resolve(&path);
    let is_root = workspace.is_repository
        && (workspace.project_root == path || workspace.worktree.as_deref() == Some(path.as_ref()));
    is_root.then_some(dir).ok_or_else(|| "ce dossier n'est pas la racine d'un dépôt Git".into())
}

/// `relative` dans `root` (canonique), liens résolus, et son chemin résolu relatif à `root`. Refusé
/// s'il en sort (`..`, chemin absolu, lien vers l'extérieur) ou s'il passe par `.git`, demandé ou
/// atteint par un lien. Contrairement à `existing_inside`, le fichier doit exister.
fn file_inside(root: &Path, relative: &str) -> CmdResult<(PathBuf, String)> {
    if !code::is_plain_relative(relative) {
        return Err("chemin hors du projet".into());
    }
    let path = root.join(relative).canonicalize().map_err(|_| "fichier introuvable : déplacé ou supprimé ?".to_owned())?;
    let resolved = path.strip_prefix(root).map_err(|_| "lien vers l'extérieur du projet : non suivi".to_owned())?;
    let resolved = resolved.to_string_lossy().into_owned();
    if !code::is_plain_relative(&resolved) {
        return Err("lien vers un chemin interdit : non suivi".into());
    }
    Ok((path, resolved))
}

/// Fichiers du projet ou du worktree, hors ignorés par git.
#[tauri::command]
pub async fn project_files(app: tauri::AppHandle, root: String) -> CmdResult<ProjectFilesDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let root = code_root(&state, &root)?;
        let files = state.vcs.files(&root.to_string_lossy()).map_err(|e| e.to_string())?;
        Ok(ProjectFilesDto::new(files))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Un fichier du projet, en lecture seule. `reveal` ouvre un fichier de secrets connu.
#[tauri::command]
pub async fn read_project_file(app: tauri::AppHandle, root: String, path: String, reveal: bool) -> CmdResult<ProjectFileDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = code_root(&app.state::<AppState>(), &root)?;
        read_inside(&root, &path, reveal)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn read_inside(root: &Path, path: &str, reveal: bool) -> CmdResult<ProjectFileDto> {
    let (file, resolved) = file_inside(root, path)?;
    // Un lien vers un fichier de secrets en est un, et l'inverse aussi.
    let named = if code::is_secret_file(path) { path } else { resolved.as_str() };
    let meta = std::fs::metadata(&file).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("pas un fichier : dossier ou sous-module".into());
    }
    let size = meta.len();
    let kind = match code::peek(named, size, reveal) {
        Peek::Secret => return Ok(ProjectFileDto::Secret),
        Peek::TooLarge { size, limit } => return Ok(ProjectFileDto::TooLarge { size, limit }),
        Peek::Read(kind) => kind,
    };
    let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
    if kind != AssetKind::Image && code::looks_binary(&bytes) {
        return Ok(ProjectFileDto::Binary { size });
    }
    Ok(ProjectFileDto::Content(AssetContentDto::of(kind, named, bytes, code::masks_tokens(named, reveal))))
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

/// Relit l'issue sur GitLab et applique les règles du domaine, comme pour une MR.
fn planned_on_issue(
    state: &AppState,
    target: &IssueRefDto,
    action: IssueActionDto,
) -> CmdResult<(robiade_core::forge::Issue, robiade_core::issue_actions::IssuePlanned)> {
    let issue = target.validate().ok_or("issue invalide")?;
    let current = state.forge.issue(&issue).map_err(|e| e.to_string())?;
    let planned = robiade_core::issue_actions::plan(&current, &action.into()).map_err(|e| e.to_string())?;
    Ok((current, planned))
}

/// Ce qu'une action sur une issue ferait, pour la confirmation. N'écrit rien.
#[tauri::command]
pub async fn preview_issue_action(
    app: tauri::AppHandle,
    target: IssueRefDto,
    action: IssueActionDto,
) -> CmdResult<IssueActionPreviewDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (issue, planned) = planned_on_issue(&state, &target, action)?;
        Ok(IssueActionPreviewDto {
            project: issue.issue.project,
            iid: issue.issue.iid,
            title: issue.title,
            state: crate::dto::issue_state(issue.state),
            assignees: issue.assignees,
            labels: issue.labels,
            action: planned.describe(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Action confirmée sur une issue : replanifiée sur l'état courant, envoyée, puis inscrite au journal.
#[tauri::command]
pub async fn run_issue_action(app: tauri::AppHandle, target: IssueRefDto, action: IssueActionDto) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (issue, planned) = planned_on_issue(&state, &target, action)?;
        let outcome = state.forge.execute_on_issue(&issue.issue, &planned).map_err(|e| e.to_string());
        let entry = robiade_core::merge_actions::AuditEntry::on_issue(now(), &issue.issue, &planned, outcome.clone());
        if let Err(e) = state.audit.record(&entry) {
            eprintln!("{e}");
        }
        outcome.map(|()| planned.describe())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ce que la création enverrait, pour la confirmation. N'écrit rien.
#[tauri::command]
pub fn preview_issue_creation(issue: NewIssueDto) -> CmdResult<String> {
    Ok(issue.validate()?.describe())
}

/// Création confirmée : revalidée, envoyée, puis inscrite au journal d'audit avec son numéro.
#[tauri::command]
pub async fn create_issue(app: tauri::AppHandle, issue: NewIssueDto) -> CmdResult<IssueRefDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let new = issue.validate()?;
        let outcome = state.forge.create_issue(&new).map_err(|e| e.to_string());
        let entry = robiade_core::merge_actions::AuditEntry::on_new_issue(now(), &new, &outcome);
        if let Err(e) = state.audit.record(&entry) {
            eprintln!("{e}");
        }
        outcome.map(|r| IssueRefDto { project: r.project, iid: r.iid })
    })
    .await
    .map_err(|e| e.to_string())?
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
        let found = state.forge.issue(&issue_ref).map_err(|e| e.to_string())?;
        let launch = Launch { repository, branch, base, prompt, tab_name: issue_start::tab_name(&found) };
        launch_in_worktree(&state, launch, cols, rows)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Dépôt principal où démarrer un agent sur la tâche, et l'issue liée, relue sur GitLab, s'il y en a
/// une. Une tâche liée part du dépôt de l'issue ; sinon, du dépôt de son propre projet.
fn task_start_context(state: &AppState, project_root: &str, task: &Task) -> CmdResult<(String, Option<Issue>)> {
    if let Some(issue) = task.issue() {
        let repository = issue_repository(state, project_root, issue)?;
        return Ok((repository, Some(state.forge.issue(issue).map_err(|e| e.to_string())?)));
    }
    let repository = main_repository(state, project_root)?;
    let own = task.project_path().and_then(|p| project_dir(p).ok());
    let own = own.map(|d| state.workspaces.resolve(&d.to_string_lossy()).project_root);
    if own.as_deref() != Some(repository.as_str()) {
        return Err("ce dossier n'est pas le dépôt du projet de la tâche".into());
    }
    Ok((repository, None))
}

/// Ce que « démarrer un agent sur cette tâche » proposera : branche, bases, prompt à relire.
#[tauri::command]
pub async fn prepare_task_start(app: tauri::AppHandle, project_root: String, id: String) -> CmdResult<IssueStartDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let task = state.tasks.get(&TaskId(id)).map_err(|e| e.to_string())?;
        let (repository, issue) = task_start_context(&state, &project_root, &task)?;
        let bases = state.worktrees.bases(&repository).map_err(|e| e.to_string())?;
        if bases.is_empty() {
            return Err("aucune base origin/staging, origin/main ni origin/master dans ce dépôt".into());
        }
        let branch = task_start::branch_name(&task, issue.as_ref());
        Ok(IssueStartDto {
            repository: repository.clone(),
            worktree_dir: issue_start::worktree_dir(&repository, &branch),
            prompt: task_start::initial_prompt(&task, issue.as_ref(), &branch),
            tab_name: task_start::tab_name(&task, issue.as_ref()),
            branch,
            bases,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Lance l'agent dans un nouveau worktree, puis lie la tâche à sa session et la passe en cours.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn start_task_agent(
    app: tauri::AppHandle,
    project_root: String,
    id: String,
    branch: String,
    base: String,
    prompt: String,
    cols: u16,
    rows: u16,
) -> CmdResult<IssueStartedDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut task = state.tasks.get(&TaskId(id)).map_err(|e| e.to_string())?;
        let (repository, issue) = task_start_context(&state, &project_root, &task)?;
        let tab_name = task_start::tab_name(&task, issue.as_ref());
        let started = launch_in_worktree(&state, Launch { repository, branch, base, prompt, tab_name }, cols, rows)?;
        // Un agent lancé ici a pour session son propre identifiant (`--session-id`).
        let session = SessionId::parse(&started.agent_id).map_err(|e| e.to_string())?;
        task.start(session, now());
        state.tasks.save(&task).map_err(|e| format!("agent lancé, mais la tâche n'a pas pu être liée : {e}"))?;
        Ok(started)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ce qu'il faut pour lancer un agent dans un nouveau worktree, une fois tout relu et validé.
struct Launch {
    repository: String,
    branch: String,
    base: String,
    prompt: String,
    tab_name: String,
}

/// Crée la branche et le worktree, puis lance l'agent avec le prompt relu et nomme son onglet.
fn launch_in_worktree(state: &AppState, launch: Launch, cols: u16, rows: u16) -> CmdResult<IssueStartedDto> {
    if !issue_start::is_valid_branch(&launch.branch) {
        return Err("nom de branche refusé".into());
    }
    let prompt = launch.prompt.trim().to_owned();
    if prompt.is_empty() || prompt.chars().count() > issue_start::PROMPT_MAX_CHARS {
        return Err("prompt vide ou trop long".into());
    }
    let dir = issue_start::worktree_dir(&launch.repository, &launch.branch);
    let fetched =
        state.worktrees.create(&launch.repository, &launch.branch, &launch.base, &dir).map_err(|e| e.to_string())?;
    let agent =
        state.agents.spawn(Path::new(&dir), None, Some(prompt), cols.clamp(20, 500), rows.clamp(5, 200))?;
    state.agents.rename(&agent_id(&agent.id)?, &launch.tab_name)?;
    Ok(IssueStartedDto { agent_id: agent.id, fetched })
}

/// Dépôt principal désigné par l'interface : ni worktree, ni sous-dossier, ni dossier hors Git.
fn main_repository(state: &AppState, raw: &str) -> CmdResult<String> {
    let dir = project_dir(raw)?.to_string_lossy().into_owned();
    let workspace = state.workspaces.resolve(&dir);
    if workspace.project_root != dir || workspace.worktree.is_some() || !workspace.is_repository {
        return Err("ce dossier n'est pas un dépôt principal".into());
    }
    Ok(dir)
}

/// Worktrees liés d'un dépôt, avec leur branche et leurs modifications en cours.
#[tauri::command]
pub async fn worktrees(app: tauri::AppHandle, repository: String) -> CmdResult<Vec<WorktreeDto>> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let repository = main_repository(&state, &repository)?;
        let found = state.worktrees.list(&repository).map_err(|e| e.to_string())?;
        Ok(found
            .into_iter()
            .map(|w| {
                let changes = if w.prunable { None } else { state.vcs.changes(&w.path).ok().map(|c| c.len()) };
                WorktreeDto::new(w, &repository, changes)
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ce qui empêche de créer le worktree : un nom de branche refusé, ou un dossier déjà là.
fn creation_blocker(branch: &str, dir: &str) -> Option<String> {
    if !issue_start::is_valid_branch(branch) {
        return Some("nom de branche refusé".into());
    }
    Path::new(dir).exists().then(|| "ce dossier existe déjà".into())
}

/// Ce que « créer un worktree » ferait : dossier exact, bases proposées, et raison d'un refus.
#[tauri::command]
pub async fn prepare_worktree_creation(
    app: tauri::AppHandle,
    repository: String,
    branch: String,
) -> CmdResult<WorktreeCreationDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let repository = main_repository(&state, &repository)?;
        let bases = state.worktrees.bases(&repository).map_err(|e| e.to_string())?;
        let dir = issue_start::worktree_dir(&repository, branch.trim());
        let blocker = if bases.is_empty() {
            Some("aucune base origin/staging, origin/main ni origin/master dans ce dépôt".into())
        } else {
            creation_blocker(branch.trim(), &dir)
        };
        Ok(WorktreeCreationDto { repository, dir, bases, blocker })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Crée la branche et son worktree sur la base mise à jour. Renvoie `false` si la base locale a servi.
#[tauri::command]
pub async fn create_worktree(app: tauri::AppHandle, repository: String, branch: String, base: String) -> CmdResult<bool> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let repository = main_repository(&state, &repository)?;
        let dir = issue_start::worktree_dir(&repository, &branch);
        if let Some(blocker) = creation_blocker(&branch, &dir) {
            return Err(blocker);
        }
        state.worktrees.create(&repository, &branch, &base, &dir).map_err(|e| e.to_string())
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

/// Fichiers en cause affichés au plus dans la confirmation ; le total reste donné.
const SHOWN_CHANGES: usize = 50;

/// Ce qui empêche de supprimer le worktree, un agent dedans, et sinon les modifications non
/// commitées que la suppression ferait perdre.
fn removal_blocker(state: &AppState, dir: &str) -> CmdResult<(Option<String>, Vec<FileChange>)> {
    if state.agents.running_in(dir) {
        return Ok((Some("un agent tourne encore dans ce worktree : arrête-le d'abord".into()), Vec::new()));
    }
    Ok((None, state.vcs.changes(dir).map_err(|e| e.to_string())?))
}

/// Ce que « supprimer le worktree » ferait : cible exacte, branche, et raison d'un refus.
#[tauri::command]
pub async fn prepare_worktree_removal(app: tauri::AppHandle, path: String) -> CmdResult<WorktreeRemovalDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (dir, repository) = linked_worktree(&state, &path)?;
        let (blocker, changes) = removal_blocker(&state, &dir)?;
        Ok(WorktreeRemovalDto {
            branch: state.worktrees.branch(&dir).map_err(|e| e.to_string())?,
            blocker,
            change_count: changes.len(),
            changes: changes.into_iter().take(SHOWN_CHANGES).map(Into::into).collect(),
            path: dir,
            repository,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Supprime le worktree ; sa branche reste. `discard` est le nombre de modifications non commitées
/// que j'ai accepté de perdre, 0 pour un arbre propre : s'il ne correspond plus à ce que contient
/// le worktree, rien n'est supprimé. Les vérifications sont refaites ici.
#[tauri::command]
pub async fn remove_worktree(app: tauri::AppHandle, path: String, discard: usize) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (dir, repository) = linked_worktree(&state, &path)?;
        let (blocker, changes) = removal_blocker(&state, &dir)?;
        if let Some(blocker) = blocker {
            return Err(blocker);
        }
        if changes.len() != discard {
            return Err(format!(
                "le worktree compte maintenant {} modification(s) non commitée(s) : relis-les avant de supprimer",
                changes.len()
            ));
        }
        state.worktrees.remove(&repository, &dir, discard > 0).map_err(|e| e.to_string())?;
        state.workspaces.forget(&dir);
        state.agents.reattach(&dir);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Montre dans le Finder un fichier d'un worktree lié, ou le worktree lui-même si `path` est vide.
/// Le chemin, relatif, doit rester dans le worktree une fois les liens résolus. Un fichier supprimé
/// n'existe plus : c'est son dossier existant le plus proche qui est montré.
#[tauri::command]
pub async fn reveal_in_finder(app: tauri::AppHandle, worktree: String, path: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (dir, _) = linked_worktree(&state, &worktree)?;
        if path.is_empty() {
            return open(&["--", &dir]);
        }
        let target = existing_inside(Path::new(&dir), &path)?;
        open(&["-R", "--", &target.to_string_lossy()])
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `relative` dans `root` (canonique), ou son élément existant le plus proche ; refusé s'il en sort
/// par `..`, par un chemin absolu ou par un dossier lien. Le dernier élément n'est pas suivi : un
/// lien (un `node_modules` partagé avec le dépôt, par exemple) est montré comme lien, pas sa cible.
fn existing_inside(root: &Path, relative: &str) -> CmdResult<PathBuf> {
    let relative = Path::new(relative);
    if !relative.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err("chemin hors du worktree".into());
    }
    let outside = || "chemin hors du worktree".to_owned();
    let found = root.join(relative);
    let found = found.ancestors().find(|p| p.symlink_metadata().is_ok()).ok_or_else(outside)?;
    let (Some(parent), Some(name)) = (found.parent(), found.file_name()) else { return Err(outside()) };
    if found == root {
        return Ok(root.to_path_buf());
    }
    let parent = parent.canonicalize().map_err(|_| outside())?;
    parent.starts_with(root).then(|| parent.join(name)).ok_or_else(outside)
}

/// `open` de macOS, lancé directement : aucun shell n'interprète les arguments.
fn open(args: &[&str]) -> CmdResult<()> {
    let status = std::process::Command::new("/usr/bin/open").args(args).status().map_err(|e| e.to_string())?;
    status.success().then_some(()).ok_or_else(|| "le Finder n'a pas pu ouvrir ce chemin".into())
}

/// Le conteneur tel que Docker le voit maintenant : une action ne part jamais sur un état périmé.
fn current_container(state: &AppState, id: &ContainerId) -> CmdResult<Container> {
    let found = state.docker.list().map_err(|e| e.to_string())?.into_iter().find(|c| c.id.matches(id));
    found.ok_or_else(|| "conteneur introuvable".into())
}

/// Rattache le conteneur au dépôt d'où son projet Compose a été lancé.
fn with_workspace(state: &AppState, c: Container) -> ContainerDto {
    let workspace = c.working_dir.as_deref().filter(|d| Path::new(d).is_absolute()).map(|d| state.workspaces.resolve(d));
    ContainerDto::new(c, workspace.as_ref())
}

fn container_action(raw: &str) -> CmdResult<ContainerAction> {
    ContainerAction::parse(raw).ok_or_else(|| "action inconnue".into())
}

/// Tous les conteneurs du démon local ; l'interface les range par projet.
#[tauri::command]
pub async fn containers(app: tauri::AppHandle) -> CmdResult<Vec<ContainerDto>> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let found = state.docker.list().map_err(|e| e.to_string())?;
        Ok(found.into_iter().map(|c| with_workspace(&state, c)).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ce que l'action ferait, sur le conteneur relu : cible exacte et raison d'un refus. N'écrit rien.
#[tauri::command]
pub async fn prepare_container_action(
    app: tauri::AppHandle,
    id: String,
    action: String,
) -> CmdResult<ContainerActionPreviewDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (id, action) = (ContainerId::parse(&id).map_err(|e| e.to_string())?, container_action(&action)?);
        let current = current_container(&state, &id)?;
        let blocker = action.refused_in(current.state).map(str::to_owned);
        Ok(ContainerActionPreviewDto { container: with_workspace(&state, current), action: action.as_str(), blocker })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Action confirmée : l'état est revérifié, puis l'action part et s'inscrit au journal d'audit.
#[tauri::command]
pub async fn run_container_action(app: tauri::AppHandle, id: String, action: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (id, action) = (ContainerId::parse(&id).map_err(|e| e.to_string())?, container_action(&action)?);
        let current = current_container(&state, &id)?;
        if let Some(refused) = action.refused_in(current.state) {
            return Err(refused.into());
        }
        let outcome = state.docker.run(&current.id, action).map_err(|e| e.to_string());
        let target = match &current.compose_project {
            Some(project) => format!("conteneur {} ({project})", current.name),
            None => format!("conteneur {}", current.name),
        };
        let result = outcome.clone().err().unwrap_or_else(|| "ok".into());
        let entry = AuditEntry { at: now(), target, action: action.as_str().into(), outcome: result };
        if let Err(e) = state.audit.record(&entry) {
            eprintln!("{e}");
        }
        outcome
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Suit les logs d'un conteneur, depuis ses dernières lignes ; renvoie l'identifiant du suivi.
#[tauri::command]
pub fn follow_container_logs(state: State<AppState>, id: String) -> CmdResult<u64> {
    let id = ContainerId::parse(&id).map_err(|e| e.to_string())?;
    state.log_streams.follow(&id, 500).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_container_logs(state: State<AppState>, stream: u64) {
    state.log_streams.stop(stream);
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

/// État de la mise à jour au lancement ; `None` tant que rien n'est décidé, ou en `tauri dev`.
#[tauri::command]
pub fn update_status(state: State<AppState>) -> Option<UpdateDto> {
    state.updates.status().as_ref().map(UpdateDto::from)
}

/// Relance l'app pour prendre la mise à jour installée. Les agents qu'elle interromprait
/// passent d'abord par la confirmation de fermeture, qui rappelle `quit` avec `relaunch`.
#[tauri::command]
pub fn relaunch(app: tauri::AppHandle) {
    if !crate::hold_quit(&app, true) {
        app.request_restart();
    }
}

/// Fermeture confirmée : les agents sont arrêtés, leurs sessions restent reprenables.
/// `relaunch` : l'app redémarre aussitôt.
#[tauri::command]
pub fn quit(app: tauri::AppHandle, state: State<AppState>, relaunch: bool) {
    state.quit_confirmed.store(true, std::sync::atomic::Ordering::SeqCst);
    state.agents.kill_all();
    if relaunch {
        app.request_restart();
    } else {
        app.exit(0);
    }
}

#[tauri::command]
pub fn db_connections(state: State<AppState>) -> CmdResult<Vec<DbConnectionDto>> {
    Ok(state.connections.all().map_err(|e| e.to_string())?.iter().map(DbConnectionDto::from).collect())
}

/// Crée ou remplace une connexion. Les règles (production en lecture seule, aucun secret en
/// clair) sont celles du domaine : le formulaire ne fait que les afficher.
#[tauri::command]
pub fn save_db_connection(state: State<AppState>, connection: DbConnectionInputDto) -> CmdResult<DbConnectionDto> {
    let stored = match connection.id {
        Some(id) => Some(state.connections.get(&ConnectionId(id)).map_err(|e| e.to_string())?),
        None => None,
    };
    let id = stored.as_ref().map_or_else(|| ConnectionId(uuid::Uuid::new_v4().to_string()), |c| c.id().clone());
    // Secret en clair laissé vide : l'interface ne l'a jamais reçu, on garde celui enregistré.
    let kept = |part: Part| {
        let fields = stored.as_ref().map_or(&[][..], |c| c.fields());
        fields.iter().find(|(p, s)| *p == part && matches!(s, Source::Literal(_))).map(|(_, s)| s.clone())
    };
    let fields = connection
        .fields
        .into_iter()
        .map(|f| {
            let (part, source) = (Part::from(f.part), Source::from(f.source));
            match source {
                Source::Literal(v) if v.is_empty() && part.is_secret() => (part, kept(part).unwrap_or(Source::Literal(v))),
                source => (part, source),
            }
        })
        .collect();
    let saved = Connection::create(
        id,
        &connection.name,
        connection.engine.into(),
        connection.stage.into(),
        connection.project_path,
        connection.read_only,
        fields,
    )
    .map_err(|e| e.to_string())?;
    state.connections.save(&saved).map_err(|e| e.to_string())?;
    // Une session ouverte garderait l'ancienne cible et l'ancienne règle de lecture seule.
    close_session(&state, &saved.id().0, "connexion modifiée");
    Ok((&saved).into())
}

#[tauri::command]
pub fn delete_db_connection(state: State<AppState>, id: String) -> CmdResult<()> {
    let deleted = state.connections.delete(&ConnectionId(id.clone())).map_err(|e| e.to_string());
    // Après la suppression : une ouverture qui relirait encore la connexion verrait sa génération périmée.
    close_session(&state, &id, "connexion supprimée");
    if deleted.is_ok() {
        state.console_history.forget(&id).map_err(|e| e.to_string())?;
    }
    deleted
}

/// Chemins des secrets d'un montage KV, sous un préfixe. Des noms seulement.
#[tauri::command]
pub async fn vault_paths(state: State<'_, AppState>, mount: String, prefix: String) -> CmdResult<Vec<String>> {
    let vault = state.vault.clone();
    tauri::async_runtime::spawn_blocking(move || vault.paths(mount.trim(), prefix.trim()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Clés d'un secret, sans leurs valeurs.
#[tauri::command]
pub async fn vault_keys(state: State<'_, AppState>, mount: String, path: String) -> CmdResult<Vec<String>> {
    let vault = state.vault.clone();
    tauri::async_runtime::spawn_blocking(move || vault.keys(mount.trim(), path.trim()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Résout chaque champ (trousseau, Vault) et assemble la cible. Les valeurs ne quittent pas Rust.
fn resolve_target(state: &AppState, connection: &Connection) -> CmdResult<Target> {
    connection.usable().map_err(|e| e.to_string())?;
    let mut values = Vec::with_capacity(connection.fields().len());
    for (part, source) in connection.fields() {
        let value = match source {
            Source::Literal(value) => value.clone(),
            Source::Keychain(service) => keychain::generic_password(service)
                .ok_or_else(|| format!("« {service} » est absent du trousseau"))?
                .expose_secret()
                .to_owned(),
            Source::Vault(reference) => state.vault.read(reference).map_err(|e| e.to_string())?,
        };
        values.push((*part, value));
    }
    Target::assemble(connection.engine(), &values).map_err(|e| e.to_string())
}

/// Où mène une connexion, sans s'y connecter : la cible relue avant de tester une production.
#[tauri::command]
pub async fn resolve_db_target(app: tauri::AppHandle, id: String) -> CmdResult<DbTargetDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let connection = state.connections.get(&ConnectionId(id)).map_err(|e| e.to_string())?;
        Ok((&resolve_target(&state, &connection)?).into())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ouvre une session, la referme, et rapporte ce que le serveur a dit, cible comprise.
#[tauri::command]
pub async fn test_db_connection(app: tauri::AppHandle, id: String) -> CmdResult<DbProbeDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let connection = state.connections.get(&ConnectionId(id)).map_err(|e| e.to_string())?;
        let target = resolve_target(&state, &connection)?;
        let tls = target.tls(connection.stage()).map_err(|e| e.to_string())?;
        let probe = state
            .databases
            .probe(connection.engine(), &target, tls, connection.read_only())
            .map_err(|e| e.to_string())?;
        // Une production qui ne serait pas en lecture seule côté serveur est un échec, pas un succès.
        if connection.read_only() && !probe.read_only {
            return Err("le serveur n'a pas appliqué la lecture seule demandée".into());
        }
        Ok(DbProbeDto::new(&target, probe))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ouvre une session de parcours, dans `database` ou à défaut la base de la connexion, et la garde
/// jusqu'à `close_db`. Une connexion en lecture seule que le serveur n'applique pas reste fermée.
#[tauri::command]
pub async fn open_db(app: tauri::AppHandle, id: String, database: Option<String>) -> CmdResult<DbOverviewDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        // Changer de base remplacerait la session, et sa transaction : on règle d'abord celle-ci.
        if state.db_sessions.get(&id).is_ok_and(|s| s.pending().is_some()) {
            return Err(DbError::ProdWritePending.to_string());
        }
        // Relevée avant de lire la connexion : une modification pendant l'ouverture l'invalide.
        let generation = state.db_sessions.generation(&id);
        let connection = state.connections.get(&ConnectionId(id.clone())).map_err(|e| e.to_string())?;
        let mut target = resolve_target(&state, &connection)?;
        if let Some(database) = database {
            // Un nom de base n'est qu'un paramètre de connexion, jamais du SQL.
            if database.is_empty() || database.chars().any(char::is_control) {
                return Err("nom de base invalide".into());
            }
            target.database = Some(database);
        }
        let language = connection.engine().language().ok_or_else(|| DbError::Unsupported("parcourir ce moteur").to_string())?;
        let tls = target.tls(connection.stage()).map_err(|e| e.to_string())?;
        let label = target_label(&target);
        let mut session = state
            .browser
            .open(connection.engine(), &target, tls, connection.read_only())
            .map_err(|e| e.to_string())?;
        let read_only = session.read_only().map_err(|e| e.to_string())?;
        if connection.read_only() && !read_only {
            return Err("le serveur n'a pas appliqué la lecture seule demandée".into());
        }
        let overview = DbOverviewDto {
            database: session.current_database().map_err(|e| e.to_string())?,
            databases: session.databases().map_err(|e| e.to_string())?,
            relations: session.relations().map_err(|e| e.to_string())?.into_iter().map(RelationDto::from).collect(),
            read_only,
        };
        let policy = Policy::of(connection.stage(), connection.read_only());
        let session = OpenSession::new(session, policy, language, read_only, connection.name().to_owned(), label);
        state.db_sessions.insert(&id, generation, session)?;
        Ok(overview)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn close_db(state: State<AppState>, id: String) {
    close_session(&state, &id, "explorateur quitté");
}

/// Une page d'une table. La table est relue au catalogue à chaque page : ses colonnes sont la
/// seule liste de noms que la requête accepte.
#[tauri::command]
pub async fn db_page(
    app: tauri::AppHandle,
    id: String,
    schema: String,
    name: String,
    request: PageRequestDto,
) -> CmdResult<PageDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let shared = state.db_sessions.get(&id)?;
        shared.run(|session| {
            let shape = session.describe(&TableRef { schema, name }).map_err(|e| e.to_string())?;
            let query = page_query(shared.language, &shape, &PageRequest::from(request)).map_err(|e| e.to_string())?;
            let rows = session.fetch(&query).map_err(|e| e.to_string())?;
            Ok(page(&shape, &query, rows).into())
        })?
    })
    .await
    .map_err(|e| e.to_string())?
}

fn target_label(t: &Target) -> String {
    let user = t.user.as_deref().map(|u| format!("{u}@")).unwrap_or_default();
    let database = t.database.as_deref().map(|d| format!("/{d}")).unwrap_or_default();
    format!("{user}{}:{}{database}", t.host, t.port)
}

/// Ferme la session d'une connexion. Une modification de prod qui attendait est annulée, et
/// l'annulation inscrite au journal.
fn close_session(state: &AppState, id: &str, why: &str) {
    if let Some((session, finished)) = state.db_sessions.close(id) {
        let outcome = finished.outcome.err().unwrap_or_else(|| format!("rollback ({why})"));
        audit_prod_write(state, &session.audit_target(), &finished.write.statement, Some(finished.write.affected), &outcome);
    }
}

fn audit_prod_write(state: &AppState, target: &str, statement: &str, affected: Option<u64>, outcome: &str) {
    if let Err(e) = state.audit.record(&AuditEntry::on_prod_write(now(), target, statement, affected, outcome)) {
        eprintln!("{e}");
    }
}

/// Exécute l'instruction sous le curseur (position de l'éditeur, en unités UTF-16), ou à défaut le
/// texte entier, qui doit alors tenir en une instruction : c'est ce qu'envoie une sélection, et ce
/// que renvoie une confirmation.
///
/// Une écriture revient d'abord à confirmer, avec l'instruction exacte. `confirmation` porte la
/// réponse : en prod, le nom de la connexion tapé à la main, sans quoi rien ne part ; une
/// modification de prod reste ensuite en attente dans sa transaction, à valider par
/// `finish_db_write`.
#[tauri::command]
pub async fn run_db_statement(
    app: tauri::AppHandle,
    id: String,
    text: String,
    cursor: Option<usize>,
    confirmation: Option<String>,
) -> CmdResult<ConsoleReplyDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let shared = state.db_sessions.get(&id)?;
        let statement = match cursor {
            Some(c) => db_console::statement_at(&text, shared.language, db_console::byte_offset(&text, c)),
            None => db_console::single_statement(&text, shared.language),
        }
        .map_err(|e| e.to_string())?
        .to_owned();
        let admission = db_console::admit(&statement, shared.language, shared.policy).map_err(|e| e.to_string())?;
        let confirm = |prod: bool| ConsoleReplyDto::Confirm {
            statement: statement.clone(),
            connection: shared.connection.clone(),
            prod,
            target: shared.target.clone(),
        };
        let kind = match admission {
            Admission::Run(kind) => kind,
            Admission::Confirm if confirmation.is_none() => return Ok(confirm(false)),
            Admission::Confirm => StatementKind::Write,
            Admission::ProdWrite => match confirmation.as_deref() {
                None => return Ok(confirm(true)),
                Some(typed) if typed != shared.connection => {
                    return Err("le nom tapé n'est pas celui de la connexion : rien n'est parti".into())
                }
                Some(_) => return begin_prod_write(&app, &state, &id, &shared, statement),
            },
        };
        if let Err(e) = state.console_history.record(&id, &statement, now()) {
            eprintln!("historique de la console non enregistré : {e}");
        }
        let (executed, elapsed_ms, still_read_only) = shared.run(|session| {
            let started = std::time::Instant::now();
            let executed = session.execute(&statement, kind, db_console::ROWS_MAX);
            let elapsed_ms = started.elapsed().as_millis() as u64;
            (executed, elapsed_ms, session.read_only().unwrap_or(false))
        })?;
        // Le serveur accepte qu'une instruction lève la lecture seule de la session : le domaine
        // refuse ce qu'il reconnaît, et ceci rattrape le reste en relisant l'état.
        if shared.read_only && !still_read_only {
            close_session(&state, &id, "lecture seule perdue");
            return Err("la session n'était plus en lecture seule : elle est fermée, rouvre la connexion".into());
        }
        let result = ConsoleResultDto::new(db_console::outcome(executed.map_err(|e| e.to_string())?), elapsed_ms);
        Ok(ConsoleReplyDto::Done { statement, result })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Exécute une modification de prod dans sa transaction et la laisse en attente : annulée au bout
/// du délai si personne ne la valide.
fn begin_prod_write(
    app: &tauri::AppHandle,
    state: &AppState,
    id: &str,
    shared: &std::sync::Arc<OpenSession>,
    statement: String,
) -> CmdResult<ConsoleReplyDto> {
    if let Err(e) = state.console_history.record(id, &statement, now()) {
        eprintln!("historique de la console non enregistré : {e}");
    }
    let target = shared.audit_target();
    let delay = std::time::Duration::from_secs(db_console::PROD_WRITE_DELAY_SECS);
    let (app, expired_id, expired_target) = (app.clone(), id.to_owned(), target.clone());
    let begun = shared.begin_write(&statement, delay, move |finished| {
        let state = app.state::<AppState>();
        let outcome = finished.outcome.err().unwrap_or_else(|| format!("rollback auto ({delay:?} sans réponse)"));
        audit_prod_write(&state, &expired_target, &finished.write.statement, Some(finished.write.affected), &outcome);
        if !finished.read_only {
            close_session(&state, &expired_id, "lecture seule perdue");
        }
    });
    match begun {
        Ok(write) => Ok(ConsoleReplyDto::Pending { statement, affected: write.affected, delay_ms: delay.as_millis() as u64 }),
        Err(e) => {
            audit_prod_write(state, &target, &statement, None, &e);
            Err(e)
        }
    }
}

/// Valide (`commit`) ou annule la modification de prod en attente, puis vérifie que la session
/// est revenue en lecture seule : sinon, elle est fermée.
#[tauri::command]
pub async fn finish_db_write(app: tauri::AppHandle, id: String, commit: bool) -> CmdResult<FinishedWriteDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let shared = state.db_sessions.get(&id)?;
        let finished = shared.finish(commit)?;
        let outcome = match &finished.outcome {
            Ok(()) if commit => "commit".to_owned(),
            Ok(()) => "rollback".to_owned(),
            Err(e) => format!("échec du {} : {e}", if commit { "commit" } else { "rollback" }),
        };
        audit_prod_write(&state, &shared.audit_target(), &finished.write.statement, Some(finished.write.affected), &outcome);
        let session_closed = !finished.read_only;
        if session_closed {
            close_session(&state, &id, "lecture seule perdue");
        }
        finished.outcome?;
        Ok(FinishedWriteDto { committed: finished.committed, affected: finished.write.affected, session_closed })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Interrompt la requête en cours de la connexion, sans attendre qu'elle rende la main.
#[tauri::command]
pub async fn cancel_db_statement(app: tauri::AppHandle, id: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().db_sessions.get(&id)?.cancel())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn db_console_history(state: State<AppState>, id: String) -> CmdResult<Vec<ConsoleHistoryDto>> {
    Ok(state.console_history.recent(&id).map_err(|e| e.to_string())?.into_iter().map(ConsoleHistoryDto::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveals_only_what_stays_inside_the_worktree() {
        let base = std::env::temp_dir().join(format!("robiade-reveal-{}", std::process::id()));
        let root = base.join("wt");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "").unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(base.join("outside/secret"), "").unwrap();
        std::os::unix::fs::symlink(base.join("outside"), root.join("node_modules")).unwrap();
        let root = root.canonicalize().unwrap();

        assert_eq!(existing_inside(&root, "src/a.rs").unwrap(), root.join("src/a.rs"));
        assert_eq!(existing_inside(&root, "").unwrap(), root);
        // Fichier supprimé : son dossier existant le plus proche.
        assert_eq!(existing_inside(&root, "src/gone/b.rs").unwrap(), root.join("src"));
        // Un lien qui sort est montré lui-même, sans être suivi.
        assert_eq!(existing_inside(&root, "node_modules").unwrap(), root.join("node_modules"));
        // Mais on ne passe pas par lui pour atteindre ce qu'il y a derrière.
        assert!(existing_inside(&root, "node_modules/secret").is_err());
        assert!(existing_inside(&root, "../outside/secret").is_err());
        assert!(existing_inside(&root, "/etc/hosts").is_err());

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn reads_only_what_stays_inside_the_project() {
        let base = std::env::temp_dir().join(format!("robiade-code-{}", std::process::id()));
        let root = base.join("projet");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(root.join("src/a.rs"), "let t = \"glpat-abcdefghijklmnopqrst\";").unwrap();
        std::fs::write(root.join(".git/config"), "[remote]").unwrap();
        std::fs::write(base.join("outside/secret"), "").unwrap();
        std::os::unix::fs::symlink(base.join("outside/secret"), root.join("fuite")).unwrap();
        std::os::unix::fs::symlink(root.join("src/a.rs"), root.join("alias.rs")).unwrap();
        std::os::unix::fs::symlink(root.join(".git/config"), root.join("config.ini")).unwrap();
        let root = root.canonicalize().unwrap();

        let text = |dto: ProjectFileDto| match dto {
            ProjectFileDto::Content(c) => c.text.unwrap(),
            _ => panic!("contenu attendu"),
        };
        // Lu, jeton masqué ; un lien qui reste dans le projet est suivi.
        assert!(text(read_inside(&root, "src/a.rs", false).unwrap()).contains("glpat-••••••"));
        assert!(text(read_inside(&root, "alias.rs", false).unwrap()).starts_with("let t"));
        for refused in ["fuite", "../outside/secret", "/etc/hosts", "src/../../outside/secret", ".git/config", ".GIT/config", "config.ini", "src"] {
            assert!(read_inside(&root, refused, false).is_err(), "{refused}");
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn does_not_load_a_secret_a_large_or_a_binary_file() {
        let root = std::env::temp_dir().join(format!("robiade-code-kinds-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".env"), "APP_SECRET=glpat-abcdefghijklmnopqrst").unwrap();
        std::fs::write(root.join("dump.sql"), vec![b'a'; code::MAX_TEXT_BYTES as usize + 1]).unwrap();
        std::fs::write(root.join("app.bin"), b"\x7fELF\0\0").unwrap();
        std::os::unix::fs::symlink(root.join(".env"), root.join("env.txt")).unwrap();
        let root = root.canonicalize().unwrap();

        assert!(matches!(read_inside(&root, ".env", false).unwrap(), ProjectFileDto::Secret));
        assert!(matches!(read_inside(&root, "env.txt", false).unwrap(), ProjectFileDto::Secret));
        // Ouvert à ma demande, il s'affiche tel quel.
        match read_inside(&root, ".env", true).unwrap() {
            ProjectFileDto::Content(c) => assert!(c.text.unwrap().contains("glpat-abcdefghijklmnopqrst")),
            _ => panic!("contenu attendu"),
        }
        assert!(matches!(read_inside(&root, "dump.sql", false).unwrap(), ProjectFileDto::TooLarge { .. }));
        assert!(matches!(read_inside(&root, "app.bin", false).unwrap(), ProjectFileDto::Binary { size: 6 }));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
