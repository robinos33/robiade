//! Objets échangés avec l'interface. Le front n'a aucune logique métier : il affiche ces DTO.

use robiade_core::agents::{Agent, AgentStatus, Gauges, Level};
use robiade_core::assets::{Asset, AssetKind};
use robiade_core::containers::{Container, ContainerState};
use robiade_core::contributions::{ContributionBucket, Granularity};
use robiade_core::databases::{Connection, Engine, Part, Probe, Source, Stage, Target, VaultRef};
use robiade_core::db_browse::{Cell, Column, Cursor, Filter, FilterOp, Page, PageRequest, Relation, RelationKind, Sort};
use robiade_core::db_console::{HistoryEntry, Outcome};
use robiade_core::forge::{
    Issue, IssueRef, IssueState, MergeRequest, MrOutcomes, MrRef, MrState, PipelineStatus, GITLAB_HOST,
};
use robiade_core::issue_actions::{IssueAction, NewIssue};
use robiade_core::project::{self, Workspace};
use robiade_core::merge_actions::{AuditEntry, MrAction};
use robiade_core::history::{EntryKind, SearchHit, SessionSummary, TokenUsage, TranscriptEntry, HIT_END, HIT_START};
use robiade_core::secrets::mask_secrets;
use robiade_core::pricing::{Estimate, PRICES_AS_OF};
use robiade_core::stats::{self, AgentsOverview, Overview};
use robiade_core::tasks::{Task, TaskStatus};
use robiade_core::vcs::{ChangeKind, FileChange, LinkedWorktree};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentOutput {
    pub id: String,
    pub data: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentDto {
    pub id: String,
    pub project_path: String,
    #[serde(flatten)]
    pub workspace: WorkspaceDto,
    pub status: &'static str,
    pub needs_attention: bool,
    pub started_at: i64,
    pub last_activity_at: i64,
    pub waited_ms: i64,
    pub permission_ms: i64,
    pub tool_calls: u32,
    pub tools: Vec<(String, u32)>,
    pub subagents: u32,
    pub files_modified: usize,
    pub last_tool: Option<String>,
    pub permission_mode: Option<String>,
    pub permissive: bool,
    pub effort: Option<String>,
    pub gauges: GaugesDto,
    pub assets: Vec<AssetDto>,
    pub resumed_from: Option<String>,
    pub name: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AssetDto {
    pub path: String,
    pub name: String,
    pub kind: &'static str,
    pub written_at: i64,
}

fn kind_name(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Image => "image",
        AssetKind::Html => "html",
        AssetKind::Markdown => "markdown",
        AssetKind::Text => "text",
    }
}

impl From<&Asset> for AssetDto {
    fn from(a: &Asset) -> Self {
        Self {
            path: a.path.clone(),
            name: a.path.rsplit('/').next().unwrap_or(&a.path).to_owned(),
            kind: kind_name(a.kind),
            written_at: a.written_at,
        }
    }
}

/// Contenu d'un asset : les images en URL `data:`, le reste en texte brut.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetContentDto {
    pub kind: &'static str,
    pub text: Option<String>,
    pub data_url: Option<String>,
}

impl AssetContentDto {
    pub fn new(asset: &Asset, bytes: Vec<u8>) -> Self {
        use base64::Engine;
        let kind = kind_name(asset.kind);
        match AssetKind::image_mime(&asset.path) {
            Some(mime) if asset.kind == AssetKind::Image => {
                let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                Self { kind, text: None, data_url: Some(format!("data:{mime};base64,{data}")) }
            }
            _ => Self { kind, text: Some(String::from_utf8_lossy(&bytes).into_owned()), data_url: None },
        }
    }
}

/// Une jauge en pourcentage, avec son niveau calculé côté Rust.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GaugeDto {
    pub percent: u8,
    pub level: &'static str,
}

impl GaugeDto {
    fn of(percent: Option<u8>) -> Option<Self> {
        let percent = percent?;
        let level = match Level::of(percent) {
            Level::Comfortable => "comfortable",
            Level::Watch => "watch",
            Level::Critical => "critical",
        };
        Some(Self { percent, level })
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GaugesDto {
    pub model: Option<String>,
    pub context: Option<GaugeDto>,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub five_hour: Option<GaugeDto>,
    pub five_hour_resets_at: Option<i64>,
    pub week: Option<GaugeDto>,
}

impl From<&Gauges> for GaugesDto {
    fn from(g: &Gauges) -> Self {
        Self {
            model: g.model.clone(),
            context: GaugeDto::of(g.context_percent),
            context_tokens: g.context_tokens,
            context_window: g.context_window,
            five_hour: GaugeDto::of(g.five_hour_percent),
            five_hour_resets_at: g.five_hour_resets_at,
            week: GaugeDto::of(g.week_percent),
        }
    }
}

/// Rattachement d'un dossier de travail : dépôt principal et worktree éventuel.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDto {
    pub project_root: String,
    pub project_name: String,
    pub in_repository: bool,
    pub worktree_path: Option<String>,
    pub worktree_name: Option<String>,
    pub gitlab_project: Option<String>,
}

impl From<&Workspace> for WorkspaceDto {
    fn from(w: &Workspace) -> Self {
        Self {
            project_root: w.project_root.clone(),
            project_name: w.project_name().to_owned(),
            in_repository: w.is_repository,
            worktree_path: w.worktree.clone(),
            worktree_name: w.worktree_name().map(str::to_owned),
            gitlab_project: w.gitlab_project.clone(),
        }
    }
}

impl AgentDto {
    pub fn from_agent(a: &Agent, workspace: &Workspace, now: i64) -> Self {
        Self {
            id: a.id().as_str().to_owned(),
            project_path: a.project_path().to_owned(),
            workspace: workspace.into(),
            status: match a.status() {
                AgentStatus::Starting => "starting",
                AgentStatus::Running => "running",
                AgentStatus::AwaitingPermission => "awaitingPermission",
                AgentStatus::AwaitingInput => "awaitingInput",
                AgentStatus::Idle => "idle",
                AgentStatus::Failed => "failed",
                AgentStatus::Ended => "ended",
            },
            needs_attention: a.status().needs_attention(),
            started_at: a.started_at(),
            last_activity_at: a.last_activity_at(),
            waited_ms: a.waited_ms(now),
            permission_ms: a.permission_ms(now),
            tool_calls: a.tool_calls(),
            tools: a.tools().into_iter().map(|(t, n)| (t.to_owned(), n)).collect(),
            subagents: a.subagents(),
            files_modified: a.files_modified(),
            last_tool: a.last_tool().map(str::to_owned),
            permission_mode: a.permission_mode().map(|m| m.as_str().to_owned()),
            permissive: a.permission_mode().is_some_and(|m| m.is_permissive()),
            effort: a.effort().map(|e| e.as_str().to_owned()),
            gauges: a.gauges().into(),
            assets: a.assets().iter().map(AssetDto::from).collect(),
            resumed_from: a.resumed_from().map(str::to_owned),
            name: a.name().map(str::to_owned),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokensDto {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub total: u64,
}

impl From<&TokenUsage> for TokensDto {
    fn from(t: &TokenUsage) -> Self {
        Self { input: t.input, output: t.output, cache_read: t.cache_read, cache_creation: t.cache_creation, total: t.total() }
    }
}

/// Coût estimé en dollars ; `unpricedTokens` compte les tokens des modèles absents de la grille.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostDto {
    pub dollars: f64,
    pub unpriced_tokens: u64,
}

impl From<Estimate> for CostDto {
    fn from(e: Estimate) -> Self {
        Self { dollars: e.dollars, unpriced_tokens: e.unpriced_tokens }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub session_id: String,
    pub project_path: String,
    #[serde(flatten)]
    pub workspace: WorkspaceDto,
    pub title: String,
    pub git_branch: Option<String>,
    pub started_at: i64,
    pub last_activity_at: i64,
    pub user_messages: u32,
    pub assistant_messages: u32,
    pub tokens: TokensDto,
    pub cost: CostDto,
    pub tool_calls: u32,
    pub models: Vec<String>,
    pub merge_request_url: Option<String>,
    pub headless: bool,
}

impl SessionDto {
    pub fn new(s: &SessionSummary, workspace: &Workspace) -> Self {
        Self {
            session_id: s.session_id.clone(),
            project_path: s.project_path.clone(),
            workspace: workspace.into(),
            title: mask_secrets(&s.title),
            git_branch: s.git_branch.clone(),
            started_at: s.started_at,
            last_activity_at: s.last_activity_at,
            user_messages: s.user_messages,
            assistant_messages: s.assistant_messages,
            tokens: (&s.tokens).into(),
            cost: stats::session_cost(s).into(),
            tool_calls: s.tools.iter().map(|(_, c)| c).sum(),
            models: s.models.clone(),
            merge_request_url: s.merge_request_url.clone(),
            headless: s.headless,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDto {
    pub kind: &'static str,
    pub at: i64,
    pub text: String,
}

impl From<TranscriptEntry> for EntryDto {
    fn from(e: TranscriptEntry) -> Self {
        let kind = match e.kind {
            EntryKind::User => "user",
            EntryKind::Assistant => "assistant",
            EntryKind::ToolUse => "tool",
        };
        Self { kind, at: e.at, text: mask_secrets(&e.text) }
    }
}

/// Extrait découpé en segments : le front surligne les passages trouvés sans jamais produire de HTML.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHitDto {
    pub session_id: String,
    /// `(texte, trouvé)` dans l'ordre de lecture.
    pub snippet: Vec<(String, bool)>,
}

impl From<SearchHit> for SearchHitDto {
    fn from(h: SearchHit) -> Self {
        // `avant␂trouvé␃après␂trouvé␃…` : le premier segment n'a jamais de passage trouvé.
        let mut snippet = vec![];
        let mut push = |text: &str, hit: bool| {
            if !text.is_empty() {
                snippet.push((text.replace(['\n', '\r'], " "), hit));
            }
        };
        let masked = mask_secrets(&h.snippet);
        let mut parts = masked.split(HIT_START);
        push(parts.next().unwrap_or_default(), false);
        for part in parts {
            let (hit, rest) = part.split_once(HIT_END).unwrap_or((part, ""));
            push(hit, true);
            push(rest, false);
        }
        Self { session_id: h.session_id, snippet }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayDto {
    pub day: i64,
    pub sessions: u32,
    pub tokens: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatDto {
    pub project_path: String,
    pub project_name: String,
    pub sessions: u32,
    pub tokens: TokensDto,
    pub cost: CostDto,
    pub last_activity_at: i64,
}

/// Mesures tirées des hooks : seulement les agents lancés dans Robiade.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsStatDto {
    pub agents: u32,
    pub failed: u32,
    pub waited_ms: i64,
    pub permission_ms: i64,
}

impl From<AgentsOverview> for AgentsStatDto {
    fn from(o: AgentsOverview) -> Self {
        Self { agents: o.agents, failed: o.failed, waited_ms: o.waited_ms, permission_ms: o.permission_ms }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatDto {
    pub model: String,
    pub tokens: TokensDto,
    pub cost: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewDto {
    pub sessions: u32,
    pub tokens: TokensDto,
    pub cost: CostDto,
    /// Date du relevé de la grille tarifaire.
    pub prices_as_of: &'static str,
    pub tool_calls: u64,
    pub merge_requests: u32,
    /// MR des sessions de la période, dont l'interface demande ensuite l'état (`merge_request_outcomes`).
    pub merge_request_refs: Vec<MrRefDto>,
    pub median_duration_ms: Option<i64>,
    pub days: Vec<DayDto>,
    pub projects: Vec<ProjectStatDto>,
    pub models: Vec<ModelStatDto>,
    pub tools: Vec<(String, u64)>,
    pub agents: AgentsStatDto,
}

impl OverviewDto {
    pub fn new(o: Overview, agents: AgentsOverview) -> Self {
        Self {
            sessions: o.sessions,
            tokens: (&o.tokens).into(),
            cost: o.cost.into(),
            prices_as_of: PRICES_AS_OF,
            tool_calls: o.tool_calls,
            median_duration_ms: o.median_duration_ms,
            merge_requests: o.merge_requests,
            merge_request_refs: o.merge_request_refs.into_iter().map(MrRefDto::from).collect(),
            days: o.days.into_iter().map(|d| DayDto { day: d.day, sessions: d.sessions, tokens: d.tokens }).collect(),
            projects: o
                .projects
                .into_iter()
                .map(|p| ProjectStatDto {
                    tokens: (&p.tokens).into(),
                    cost: p.cost.into(),
                    project_name: project::display_name(&p.project_path).to_owned(),
                    project_path: p.project_path,
                    sessions: p.sessions,
                    last_activity_at: p.last_activity_at,
                })
                .collect(),
            models: o
                .models
                .into_iter()
                .map(|m| ModelStatDto { model: m.model, tokens: (&m.tokens).into(), cost: m.cost })
                .collect(),
            tools: o.tools.into_iter().take(12).collect(),
            agents: agents.into(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeDto {
    pub path: String,
    pub kind: &'static str,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

impl From<FileChange> for FileChangeDto {
    fn from(c: FileChange) -> Self {
        let kind = match c.kind {
            ChangeKind::Modified => "modified",
            ChangeKind::Added => "added",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Renamed => "renamed",
            ChangeKind::Untracked => "untracked",
        };
        Self { path: c.path, kind, added: c.added, removed: c.removed }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatusDto {
    Todo,
    Doing,
    Done,
}

impl From<TaskStatus> for TaskStatusDto {
    fn from(s: TaskStatus) -> Self {
        match s {
            TaskStatus::Todo => Self::Todo,
            TaskStatus::Doing => Self::Doing,
            TaskStatus::Done => Self::Done,
        }
    }
}

impl From<TaskStatusDto> for TaskStatus {
    fn from(s: TaskStatusDto) -> Self {
        match s {
            TaskStatusDto::Todo => Self::Todo,
            TaskStatusDto::Doing => Self::Doing,
            TaskStatusDto::Done => Self::Done,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDto {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub project_path: Option<String>,
    pub status: TaskStatusDto,
    /// `AAAA-MM-JJ`.
    pub due_on: Option<String>,
    pub issue: Option<IssueRefDto>,
    /// Adresse GitLab de l'issue liée, pour l'ouvrir.
    pub issue_url: Option<String>,
    pub session_id: Option<String>,
    pub created_at: i64,
    pub done_at: Option<i64>,
}

impl From<&Task> for TaskDto {
    fn from(t: &Task) -> Self {
        Self {
            id: t.id().0.clone(),
            title: t.title().to_owned(),
            notes: t.notes().to_owned(),
            project_path: t.project_path().map(str::to_owned),
            status: t.status().into(),
            due_on: t.due_on().map(|d| d.as_str().to_owned()),
            issue: t.issue().map(|i| IssueRefDto { project: i.project.clone(), iid: i.iid }),
            issue_url: t.issue().map(IssueRef::web_url),
            session_id: t.session().map(|s| s.as_str().to_owned()),
            created_at: t.created_at(),
            done_at: t.done_at(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueRefDto {
    pub project: String,
    pub iid: u64,
}

impl IssueRefDto {
    /// Revalidée comme une URL d'issue de l'instance.
    pub fn validate(&self) -> Option<IssueRef> {
        IssueRef::from_url(&format!("https://{GITLAB_HOST}/{}/-/issues/{}", self.project, self.iid))
            .filter(|r| r.project == self.project)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueStartDto {
    pub repository: String,
    pub branch: String,
    pub bases: Vec<String>,
    pub worktree_dir: String,
    pub prompt: String,
    pub tab_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueStartedDto {
    pub agent_id: String,
    /// La base a été mise à jour depuis origin ; sinon, la base locale a servi.
    pub fetched: bool,
}

/// Un worktree lié d'un dépôt, avec ce qui décide de sa suppression : modifications et verrou.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDto {
    pub path: String,
    pub name: String,
    pub repository: String,
    /// `None` : tête détachée.
    pub branch: Option<String>,
    /// Fichiers modifiés ou non suivis ; `None` si le dossier est illisible ou disparu.
    pub changes: Option<usize>,
    pub locked: bool,
    pub prunable: bool,
}

impl WorktreeDto {
    pub fn new(w: LinkedWorktree, repository: &str, changes: Option<usize>) -> Self {
        Self {
            name: project::display_name(&w.path).to_owned(),
            repository: repository.to_owned(),
            changes,
            branch: w.branch,
            locked: w.locked,
            prunable: w.prunable,
            path: w.path,
        }
    }
}

/// Ce que « créer un worktree » ferait, montré avant de confirmer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreationDto {
    pub repository: String,
    pub dir: String,
    pub bases: Vec<String>,
    /// Raison du refus ; `None` si la création est possible.
    pub blocker: Option<String>,
}

/// Ce que « supprimer le worktree » ferait, montré avant de confirmer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRemovalDto {
    pub path: String,
    pub repository: String,
    /// `None` : tête détachée.
    pub branch: Option<String>,
    /// Raison du refus (un agent dedans) ; `None` si la suppression est possible.
    pub blocker: Option<String>,
    /// Fichiers modifiés ou non suivis que la suppression ferait perdre, les premiers seulement ;
    /// `change_count` les compte tous.
    pub changes: Vec<FileChangeDto>,
    pub change_count: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrRefDto {
    pub project: String,
    pub iid: u64,
}

impl From<MrRef> for MrRefDto {
    fn from(r: MrRef) -> Self {
        Self { project: r.project, iid: r.iid }
    }
}

impl MrRefDto {
    /// Revalidée comme une URL de MR de l'instance : le front ne peut désigner rien d'autre.
    pub fn validate(&self) -> Option<MrRef> {
        MrRef::from_url(&format!("https://{GITLAB_HOST}/{}/-/merge_requests/{}", self.project, self.iid))
            .filter(|r| r.project == self.project)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRequestDto {
    pub project: String,
    pub iid: u64,
    pub title: String,
    pub author: String,
    pub state: &'static str,
    pub source_branch: String,
    pub target_branch: String,
    pub web_url: String,
    pub updated_at: i64,
    /// Commit de tête vu à l'affichage : le merge n'est accepté que sur lui.
    pub sha: String,
    pub has_conflicts: bool,
    pub unresolved_discussions: bool,
    pub merge_status: String,
    pub pipeline: Option<&'static str>,
    pub wagon: bool,
    pub approvals: Option<ApprovalsDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalsDto {
    pub required: u32,
    pub left: u32,
    pub by: Vec<String>,
}

impl From<MergeRequest> for MergeRequestDto {
    fn from(m: MergeRequest) -> Self {
        let wagon = m.is_intermediate_wagon();
        Self {
            project: m.mr.project,
            iid: m.mr.iid,
            title: m.title,
            author: m.author,
            state: mr_state(m.state),
            source_branch: m.source_branch,
            target_branch: m.target_branch,
            web_url: m.web_url,
            updated_at: m.updated_at,
            sha: m.sha,
            has_conflicts: m.has_conflicts,
            unresolved_discussions: m.unresolved_discussions,
            merge_status: m.merge_status,
            pipeline: m.pipeline.map(|p| match p {
                PipelineStatus::Running => "running",
                PipelineStatus::Success => "success",
                PipelineStatus::Failed => "failed",
                PipelineStatus::Manual => "manual",
                PipelineStatus::Canceled => "canceled",
            }),
            wagon,
            approvals: m.approvals.map(|a| ApprovalsDto { required: a.required, left: a.left, by: a.by }),
        }
    }
}

pub fn mr_state(state: MrState) -> &'static str {
    match state {
        MrState::Draft => "draft",
        MrState::Opened => "opened",
        MrState::Merged => "merged",
        MrState::Closed => "closed",
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDto {
    pub project: String,
    pub iid: u64,
    pub title: String,
    pub state: &'static str,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub web_url: String,
    pub updated_at: i64,
    pub due_date: Option<String>,
    pub merge_requests: u32,
}

impl From<Issue> for IssueDto {
    fn from(i: Issue) -> Self {
        Self {
            project: i.issue.project,
            iid: i.issue.iid,
            title: i.title,
            state: issue_state(i.state),
            labels: i.labels,
            assignees: i.assignees,
            web_url: i.web_url,
            updated_at: i.updated_at,
            due_date: i.due_date,
            merge_requests: i.merge_requests,
        }
    }
}

pub fn issue_state(state: IssueState) -> &'static str {
    match state {
        IssueState::Opened => "opened",
        IssueState::Closed => "closed",
    }
}

/// Action demandée par l'interface ; les règles s'appliquent ensuite côté domaine.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MrActionDto {
    Comment { body: String },
    Approve,
    Rebase,
    RetryPipeline,
    MarkReady,
    MarkDraft,
    Merge {
        #[serde(rename = "expectedSha")]
        expected_sha: String,
    },
}

impl From<MrActionDto> for MrAction {
    fn from(a: MrActionDto) -> Self {
        match a {
            MrActionDto::Comment { body } => MrAction::Comment(body),
            MrActionDto::Approve => MrAction::Approve,
            MrActionDto::Rebase => MrAction::Rebase,
            MrActionDto::RetryPipeline => MrAction::RetryPipeline,
            MrActionDto::MarkReady => MrAction::MarkReady,
            MrActionDto::MarkDraft => MrAction::MarkDraft,
            MrActionDto::Merge { expected_sha } => MrAction::Merge { expected_sha },
        }
    }
}

/// Action demandée sur une issue ; les règles s'appliquent ensuite côté domaine.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum IssueActionDto {
    Close,
    Reopen,
    Assign { assignees: Vec<String> },
    Label { labels: Vec<String> },
}

impl From<IssueActionDto> for IssueAction {
    fn from(a: IssueActionDto) -> Self {
        match a {
            IssueActionDto::Close => IssueAction::Close,
            IssueActionDto::Reopen => IssueAction::Reopen,
            IssueActionDto::Assign { assignees } => IssueAction::Assign(assignees),
            IssueActionDto::Label { labels } => IssueAction::Label(labels),
        }
    }
}

/// Issue à créer, telle que saisie ; validée ensuite par le domaine.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewIssueDto {
    pub project: String,
    pub title: String,
    pub description: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
}

impl NewIssueDto {
    pub fn validate(&self) -> Result<NewIssue, String> {
        NewIssue::new(&self.project, &self.title, &self.description, &self.labels, &self.assignees)
            .map_err(|e| e.to_string())
    }
}

/// Confirmation d'une action sur une issue : l'issue relue sur GitLab, et ce qui partira.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueActionPreviewDto {
    pub project: String,
    pub iid: u64,
    pub title: String,
    pub state: &'static str,
    pub assignees: Vec<String>,
    pub labels: Vec<String>,
    pub action: String,
}

/// Ce que la confirmation montre : la cible exacte, relue sur GitLab, et ce qui partira.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionPreviewDto {
    pub project: String,
    pub iid: u64,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub author: String,
    pub action: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntryDto {
    pub at: i64,
    pub target: String,
    pub action: String,
    pub outcome: String,
}

impl From<AuditEntry> for AuditEntryDto {
    fn from(e: AuditEntry) -> Self {
        Self { at: e.at, target: e.target, action: e.action, outcome: e.outcome }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeDto {
    pub mine: Vec<MergeRequestDto>,
    pub reviews: Vec<MergeRequestDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MrOutcomesDto {
    pub total: usize,
    pub merged: usize,
    pub opened: usize,
    pub closed: usize,
    pub unknown: usize,
}

impl From<MrOutcomes> for MrOutcomesDto {
    fn from(o: MrOutcomes) -> Self {
        Self { total: o.total, merged: o.merged, opened: o.opened, closed: o.closed, unknown: o.unknown }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum GranularityDto {
    Day,
    Week,
    Month,
    Year,
}

impl From<GranularityDto> for Granularity {
    fn from(g: GranularityDto) -> Self {
        match g {
            GranularityDto::Day => Self::Day,
            GranularityDto::Week => Self::Week,
            GranularityDto::Month => Self::Month,
            GranularityDto::Year => Self::Year,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributionBucketDto {
    /// Premier jour local de la période, en jours depuis l'epoch.
    pub start_day: i64,
    pub mrs_opened: u32,
    pub mrs_merged: u32,
    pub commits: u32,
    pub reviews: u32,
}

impl From<ContributionBucket> for ContributionBucketDto {
    fn from(b: ContributionBucket) -> Self {
        Self { start_day: b.start_day, mrs_opened: b.mrs_opened, mrs_merged: b.mrs_merged, commits: b.commits, reviews: b.reviews }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDto {
    pub claude_path: Option<String>,
    pub home: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContainerLogs {
    pub stream: u64,
    /// Octets en base64 ; `None` quand le flux est terminé.
    pub data: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortDto {
    pub private: u16,
    pub public: Option<u16>,
    pub protocol: String,
}

/// Un conteneur, et le dépôt d'où son projet Compose a été lancé (`None` hors Compose).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerDto {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: &'static str,
    pub status: String,
    pub ports: Vec<PortDto>,
    pub compose_project: Option<String>,
    pub service: Option<String>,
    pub working_dir: Option<String>,
    pub workspace: Option<WorkspaceDto>,
}

pub fn container_state(state: ContainerState) -> &'static str {
    match state {
        ContainerState::Created => "created",
        ContainerState::Running => "running",
        ContainerState::Paused => "paused",
        ContainerState::Restarting => "restarting",
        ContainerState::Exited => "exited",
        ContainerState::Dead => "dead",
        ContainerState::Other => "other",
    }
}

impl ContainerDto {
    pub fn new(c: Container, workspace: Option<&Workspace>) -> Self {
        Self {
            id: c.id.as_str().to_owned(),
            name: c.name,
            image: c.image,
            state: container_state(c.state),
            status: c.status,
            ports: c.ports.into_iter().map(|p| PortDto { private: p.private, public: p.public, protocol: p.protocol }).collect(),
            compose_project: c.compose_project,
            service: c.service,
            working_dir: c.working_dir,
            workspace: workspace.map(WorkspaceDto::from),
        }
    }
}

/// Ce que la confirmation d'une action sur un conteneur montre, relu sur Docker juste avant.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerActionPreviewDto {
    pub container: ContainerDto,
    pub action: &'static str,
    /// Raison du refus ; `None` si l'action est possible.
    pub blocker: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum EngineDto {
    Postgres,
    Mariadb,
    Mongo,
    Elasticsearch,
}

impl From<Engine> for EngineDto {
    fn from(e: Engine) -> Self {
        match e {
            Engine::Postgres => Self::Postgres,
            Engine::MariaDb => Self::Mariadb,
            Engine::Mongo => Self::Mongo,
            Engine::Elasticsearch => Self::Elasticsearch,
        }
    }
}

impl From<EngineDto> for Engine {
    fn from(e: EngineDto) -> Self {
        match e {
            EngineDto::Postgres => Self::Postgres,
            EngineDto::Mariadb => Self::MariaDb,
            EngineDto::Mongo => Self::Mongo,
            EngineDto::Elasticsearch => Self::Elasticsearch,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum StageDto {
    Dev,
    Staging,
    Prod,
}

impl From<Stage> for StageDto {
    fn from(s: Stage) -> Self {
        match s {
            Stage::Dev => Self::Dev,
            Stage::Staging => Self::Staging,
            Stage::Prod => Self::Prod,
        }
    }
}

impl From<StageDto> for Stage {
    fn from(s: StageDto) -> Self {
        match s {
            StageDto::Dev => Self::Dev,
            StageDto::Staging => Self::Staging,
            StageDto::Prod => Self::Prod,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum PartDto {
    Url,
    Host,
    Port,
    Database,
    User,
    Password,
}

impl From<Part> for PartDto {
    fn from(p: Part) -> Self {
        match p {
            Part::Url => Self::Url,
            Part::Host => Self::Host,
            Part::Port => Self::Port,
            Part::Database => Self::Database,
            Part::User => Self::User,
            Part::Password => Self::Password,
        }
    }
}

impl From<PartDto> for Part {
    fn from(p: PartDto) -> Self {
        match p {
            PartDto::Url => Self::Url,
            PartDto::Host => Self::Host,
            PartDto::Port => Self::Port,
            PartDto::Database => Self::Database,
            PartDto::User => Self::User,
            PartDto::Password => Self::Password,
        }
    }
}

/// D'où vient la valeur d'un champ. Seul `literal` porte une valeur ; vers l'interface, celle d'un
/// champ secret part vide (voir `FieldDto::masked`), et revient vide quand elle n'a pas changé.
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SourceDto {
    Literal { value: String },
    Keychain { service: String },
    Vault { mount: String, path: String, key: String },
}

impl From<&Source> for SourceDto {
    fn from(s: &Source) -> Self {
        match s {
            Source::Literal(value) => Self::Literal { value: value.clone() },
            Source::Keychain(service) => Self::Keychain { service: service.clone() },
            Source::Vault(r) => Self::Vault { mount: r.mount.clone(), path: r.path.clone(), key: r.key.clone() },
        }
    }
}

impl From<SourceDto> for Source {
    fn from(s: SourceDto) -> Self {
        match s {
            SourceDto::Literal { value } => Self::Literal(value),
            SourceDto::Keychain { service } => Self::Keychain(service),
            SourceDto::Vault { mount, path, key } => Self::Vault(VaultRef { mount, path, key }),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FieldDto {
    pub part: PartDto,
    pub source: SourceDto,
}

impl FieldDto {
    /// Un mot de passe en clair ne repart jamais vers l'interface une fois enregistré.
    fn masked(part: Part, source: &Source) -> Self {
        let source = match source {
            Source::Literal(_) if part.is_secret() => SourceDto::Literal { value: String::new() },
            other => other.into(),
        };
        Self { part: part.into(), source }
    }
}

/// Une connexion telle que le formulaire l'envoie ; `id` absent pour une création.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbConnectionInputDto {
    pub id: Option<String>,
    pub name: String,
    pub engine: EngineDto,
    pub stage: StageDto,
    pub project_path: Option<String>,
    pub read_only: bool,
    pub fields: Vec<FieldDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbConnectionDto {
    pub id: String,
    pub name: String,
    pub engine: EngineDto,
    pub stage: StageDto,
    pub project_path: Option<String>,
    pub read_only: bool,
    pub fields: Vec<FieldDto>,
}

impl From<&Connection> for DbConnectionDto {
    fn from(c: &Connection) -> Self {
        Self {
            id: c.id().0.clone(),
            name: c.name().to_owned(),
            engine: c.engine().into(),
            stage: c.stage().into(),
            project_path: c.project_path().map(str::to_owned),
            read_only: c.read_only(),
            fields: c.fields().iter().map(|(p, s)| FieldDto::masked(*p, s)).collect(),
        }
    }
}

/// Où une connexion mène, valeurs résolues, sans le mot de passe : de quoi relire la cible.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbTargetDto {
    pub host: String,
    pub port: u16,
    pub database: Option<String>,
    pub user: Option<String>,
    pub ssl_mode: Option<String>,
}

impl From<&Target> for DbTargetDto {
    fn from(t: &Target) -> Self {
        Self {
            host: t.host.clone(),
            port: t.port,
            database: t.database.clone(),
            user: t.user.clone(),
            ssl_mode: t.ssl_mode.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbProbeDto {
    pub target: DbTargetDto,
    pub server_version: String,
    pub user: String,
    pub database: String,
    pub read_only: bool,
}

impl DbProbeDto {
    pub fn new(target: &Target, p: Probe) -> Self {
        Self { target: target.into(), server_version: p.server_version, user: p.user, database: p.database, read_only: p.read_only }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RelationKindDto {
    Table,
    View,
    MaterializedView,
    Foreign,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationDto {
    pub schema: String,
    pub name: String,
    pub kind: RelationKindDto,
    pub estimated_rows: Option<i64>,
}

impl From<Relation> for RelationDto {
    fn from(r: Relation) -> Self {
        let kind = match r.kind {
            RelationKind::Table => RelationKindDto::Table,
            RelationKind::View => RelationKindDto::View,
            RelationKind::MaterializedView => RelationKindDto::MaterializedView,
            RelationKind::Foreign => RelationKindDto::Foreign,
        };
        Self { schema: r.table.schema, name: r.table.name, kind, estimated_rows: r.estimated_rows }
    }
}

/// Une session ouverte : la base où elle est, les autres bases du serveur, et ce qu'on peut y lire.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbOverviewDto {
    pub database: String,
    pub databases: Vec<String>,
    pub relations: Vec<RelationDto>,
    /// Lu sur le serveur.
    pub read_only: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDto {
    pub name: String,
    pub data_type: String,
    pub primary_key: Option<u16>,
}

impl From<Column> for ColumnDto {
    fn from(c: Column) -> Self {
        Self { name: c.name, data_type: c.data_type, primary_key: c.primary_key }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDto {
    pub text: String,
    pub truncated: bool,
}

/// Opaque pour l'interface : elle renvoie tel quel celui que la page précédente lui a donné.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CursorDto {
    Start,
    After { values: Vec<String> },
    Offset { offset: u64 },
}

impl From<Cursor> for CursorDto {
    fn from(c: Cursor) -> Self {
        match c {
            Cursor::Start => Self::Start,
            Cursor::After(values) => Self::After { values },
            Cursor::Offset(offset) => Self::Offset { offset },
        }
    }
}

impl From<CursorDto> for Cursor {
    fn from(c: CursorDto) -> Self {
        match c {
            CursorDto::Start => Self::Start,
            CursorDto::After { values } => Self::After(values),
            CursorDto::Offset { offset } => Self::Offset(offset),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum FilterOpDto {
    Equals { value: String },
    Contains { value: String },
    IsNull,
    IsNotNull,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterDto {
    pub column: String,
    #[serde(flatten)]
    pub op: FilterOpDto,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortDto {
    pub column: String,
    pub descending: bool,
}

/// Une page demandée par la grille : des noms et des valeurs, jamais de SQL.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRequestDto {
    pub sort: Option<SortDto>,
    pub filters: Vec<FilterDto>,
    pub cursor: CursorDto,
}

impl From<PageRequestDto> for PageRequest {
    fn from(r: PageRequestDto) -> Self {
        let filters = r
            .filters
            .into_iter()
            .map(|f| Filter {
                column: f.column,
                op: match f.op {
                    FilterOpDto::Equals { value } => FilterOp::Equals(value),
                    FilterOpDto::Contains { value } => FilterOp::Contains(value),
                    FilterOpDto::IsNull => FilterOp::IsNull,
                    FilterOpDto::IsNotNull => FilterOp::IsNotNull,
                },
            })
            .collect();
        Self {
            sort: r.sort.map(|s| Sort { column: s.column, descending: s.descending }),
            filters,
            cursor: r.cursor.into(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDto {
    pub columns: Vec<ColumnDto>,
    pub rows: Vec<Vec<Option<CellDto>>>,
    pub next: Option<CursorDto>,
}

impl From<Page> for PageDto {
    fn from(p: Page) -> Self {
        let cell = |c: Option<Cell>| c.map(|c| CellDto { text: c.text, truncated: c.truncated });
        Self {
            columns: p.columns.into_iter().map(ColumnDto::from).collect(),
            rows: p.rows.into_iter().map(|row| row.into_iter().map(cell).collect()).collect(),
            next: p.next.map(CursorDto::from),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleResultDto {
    pub columns: Vec<ColumnDto>,
    pub rows: Vec<Vec<Option<CellDto>>>,
    pub truncated: bool,
    pub affected: Option<u64>,
    pub elapsed_ms: u64,
}

impl ConsoleResultDto {
    pub fn new(o: Outcome, elapsed_ms: u64) -> Self {
        let cell = |c: Option<Cell>| c.map(|c| CellDto { text: c.text, truncated: c.truncated });
        Self {
            columns: o.columns.into_iter().map(ColumnDto::from).collect(),
            rows: o.rows.into_iter().map(|row| row.into_iter().map(cell).collect()).collect(),
            truncated: o.truncated,
            affected: o.affected,
            elapsed_ms,
        }
    }
}

/// Ce que la console reçoit : une écriture à confirmer, ou le résultat.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConsoleReplyDto {
    /// `prod` : la confirmation exige de taper le nom de la connexion.
    #[serde(rename_all = "camelCase")]
    Confirm { statement: String, connection: String, prod: bool, target: String },
    #[serde(rename_all = "camelCase")]
    Done { statement: String, result: ConsoleResultDto },
    /// Modification de prod exécutée dans sa transaction, à valider ou annuler avant `delay_ms`.
    #[serde(rename_all = "camelCase")]
    Pending { statement: String, affected: u64, delay_ms: u64 },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedWriteDto {
    pub committed: bool,
    pub affected: u64,
    /// La session n'était plus en lecture seule après coup : elle est fermée.
    pub session_closed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleHistoryDto {
    pub sql: String,
    pub at: i64,
}

impl From<HistoryEntry> for ConsoleHistoryDto {
    fn from(h: HistoryEntry) -> Self {
        Self { sql: h.sql, at: h.at }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_page_request_from_the_grid() {
        let json = r#"{
            "sort": { "column": "email", "descending": true },
            "filters": [
                { "column": "id", "op": "equals", "value": "7" },
                { "column": "email", "op": "contains", "value": "@ex" },
                { "column": "avatar", "op": "isNull" }
            ],
            "cursor": { "kind": "after", "values": ["42"] }
        }"#;
        let request = PageRequest::from(serde_json::from_str::<PageRequestDto>(json).unwrap());
        assert_eq!(request.sort, Some(Sort { column: "email".into(), descending: true }));
        assert_eq!(request.filters[0], Filter { column: "id".into(), op: FilterOp::Equals("7".into()) });
        assert_eq!(request.filters[2].op, FilterOp::IsNull);
        assert_eq!(request.cursor, Cursor::After(vec!["42".into()]));
        let next = serde_json::to_value(CursorDto::from(Cursor::Offset(400))).unwrap();
        assert_eq!(next, serde_json::json!({ "kind": "offset", "offset": 400 }));
    }

    #[test]
    fn splits_a_snippet_into_highlighted_segments() {
        let hit = SearchHit { session_id: "s".into(), snippet: "le \u{2}rebase\u{3} du\n\u{2}wagon\u{3}".into() };
        let dto = SearchHitDto::from(hit);
        let expected = [("le ", false), ("rebase", true), (" du ", false), ("wagon", true)];
        assert_eq!(dto.snippet, expected.map(|(t, h)| (t.to_owned(), h)));
    }

    #[test]
    fn masks_a_token_found_by_the_search() {
        let hit = SearchHit { session_id: "s".into(), snippet: "TOKEN=\u{2}glpat\u{3}-AbCdEf0123456789xyzw".into() };
        let dto = SearchHitDto::from(hit);
        let expected = [("TOKEN=", false), ("glpat", true), ("-••••••", false)];
        assert_eq!(dto.snippet, expected.map(|(t, h)| (t.to_owned(), h)));
    }
}
