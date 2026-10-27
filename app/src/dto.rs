//! Objets échangés avec l'interface. Le front n'a aucune logique métier : il affiche ces DTO.

use robiade_core::agents::{Agent, AgentStatus};
use robiade_core::project;
use robiade_core::history::{EntryKind, SessionSummary, TokenUsage, TranscriptEntry};
use robiade_core::stats::Overview;
use robiade_core::tasks::{Task, TaskStatus};
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
    pub project_root: String,
    pub project_name: String,
    pub status: &'static str,
    pub needs_attention: bool,
    pub started_at: i64,
    pub last_activity_at: i64,
    pub waited_ms: i64,
    pub tool_calls: u32,
    pub last_tool: Option<String>,
    pub permission_mode: Option<String>,
    pub permissive: bool,
    pub resumed_from: Option<String>,
}

impl AgentDto {
    pub fn from_agent(a: &Agent, now: i64) -> Self {
        Self {
            id: a.id().as_str().to_owned(),
            project_path: a.project_path().to_owned(),
            project_root: project::root_of(a.project_path()).to_owned(),
            project_name: project::display_name(a.project_path()).to_owned(),
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
            tool_calls: a.tool_calls(),
            last_tool: a.last_tool().map(str::to_owned),
            permission_mode: a.permission_mode().map(|m| m.as_str().to_owned()),
            permissive: a.permission_mode().is_some_and(|m| m.is_permissive()),
            resumed_from: a.resumed_from().map(str::to_owned),
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub session_id: String,
    pub project_path: String,
    pub project_root: String,
    pub project_name: String,
    pub title: String,
    pub git_branch: Option<String>,
    pub started_at: i64,
    pub last_activity_at: i64,
    pub user_messages: u32,
    pub assistant_messages: u32,
    pub tokens: TokensDto,
    pub tool_calls: u32,
    pub models: Vec<String>,
    pub merge_request_url: Option<String>,
}

impl From<&SessionSummary> for SessionDto {
    fn from(s: &SessionSummary) -> Self {
        Self {
            session_id: s.session_id.clone(),
            project_path: s.project_path.clone(),
            project_root: project::root_of(&s.project_path).to_owned(),
            project_name: project::display_name(&s.project_path).to_owned(),
            title: s.title.clone(),
            git_branch: s.git_branch.clone(),
            started_at: s.started_at,
            last_activity_at: s.last_activity_at,
            user_messages: s.user_messages,
            assistant_messages: s.assistant_messages,
            tokens: (&s.tokens).into(),
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
    pub last_activity_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewDto {
    pub sessions: u32,
    pub tokens: TokensDto,
    pub tool_calls: u64,
    pub merge_requests: u32,
    pub days: Vec<DayDto>,
    pub projects: Vec<ProjectStatDto>,
    pub tools: Vec<(String, u64)>,
}

impl From<Overview> for OverviewDto {
    fn from(o: Overview) -> Self {
        Self {
            sessions: o.sessions,
            tokens: (&o.tokens).into(),
            tool_calls: o.tool_calls,
            merge_requests: o.merge_requests,
            days: o.days.into_iter().map(|d| DayDto { day: d.day, sessions: d.sessions, tokens: d.tokens }).collect(),
            projects: o
                .projects
                .into_iter()
                .map(|p| ProjectStatDto {
                    tokens: (&p.tokens).into(),
                    project_name: project::display_name(&p.project_path).to_owned(),
                    project_path: p.project_path,
                    sessions: p.sessions,
                    last_activity_at: p.last_activity_at,
                })
                .collect(),
            tools: o.tools.into_iter().take(12).collect(),
        }
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDto {
    pub claude_path: Option<String>,
    pub home: String,
}
