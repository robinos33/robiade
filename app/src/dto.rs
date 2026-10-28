//! Objets échangés avec l'interface. Le front n'a aucune logique métier : il affiche ces DTO.

use robiade_core::agents::{Agent, AgentStatus, Gauges, Level};
use robiade_core::assets::{Asset, AssetKind};
use robiade_core::forge::{MergeRequest, MrOutcomes, MrRef, MrState, PipelineStatus, GITLAB_HOST};
use robiade_core::project::{self, Workspace};
use robiade_core::history::{EntryKind, SearchHit, SessionSummary, TokenUsage, TranscriptEntry, HIT_END, HIT_START};
use robiade_core::pricing::{Estimate, PRICES_AS_OF};
use robiade_core::stats::{self, AgentsOverview, Overview};
use robiade_core::tasks::{Task, TaskStatus};
use robiade_core::vcs::{ChangeKind, FileChange};
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
}

impl From<&Workspace> for WorkspaceDto {
    fn from(w: &Workspace) -> Self {
        Self {
            project_root: w.project_root.clone(),
            project_name: w.project_name().to_owned(),
            in_repository: w.is_repository,
            worktree_path: w.worktree.clone(),
            worktree_name: w.worktree_name().map(str::to_owned),
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
}

impl SessionDto {
    pub fn new(s: &SessionSummary, workspace: &Workspace) -> Self {
        Self {
            session_id: s.session_id.clone(),
            project_path: s.project_path.clone(),
            workspace: workspace.into(),
            title: s.title.clone(),
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
        Self { kind, at: e.at, text: e.text }
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
        let mut parts = h.snippet.split(HIT_START);
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
            created_at: t.created_at(),
            done_at: t.done_at(),
        }
    }
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
    pub has_conflicts: bool,
    pub unresolved_discussions: bool,
    pub merge_status: String,
    pub pipeline: Option<&'static str>,
    pub wagon: bool,
}

impl From<MergeRequest> for MergeRequestDto {
    fn from(m: MergeRequest) -> Self {
        let wagon = m.is_intermediate_wagon();
        Self {
            project: m.mr.project,
            iid: m.mr.iid,
            title: m.title,
            author: m.author,
            state: match m.state {
                MrState::Draft => "draft",
                MrState::Opened => "opened",
                MrState::Merged => "merged",
                MrState::Closed => "closed",
            },
            source_branch: m.source_branch,
            target_branch: m.target_branch,
            web_url: m.web_url,
            updated_at: m.updated_at,
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
        }
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDto {
    pub claude_path: Option<String>,
    pub home: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_snippet_into_highlighted_segments() {
        let hit = SearchHit { session_id: "s".into(), snippet: "le \u{2}rebase\u{3} du\n\u{2}wagon\u{3}".into() };
        let dto = SearchHitDto::from(hit);
        let expected = [("le ", false), ("rebase", true), (" du ", false), ("wagon", true)];
        assert_eq!(dto.snippet, expected.map(|(t, h)| (t.to_owned(), h)));
    }
}
