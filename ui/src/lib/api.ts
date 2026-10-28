import { invoke } from '@tauri-apps/api/core';
import type {
  Agent, AssetContent, Container, ContainerAction, ContainerActionPreview, Entry, FileChange, Environment, ActionPreview, AuditEntry, Forge, Issue, IssueAction, IssueActionPreview, IssueStart, MrAction, NewIssue, MrOutcomes, MrRef, MrState, Overview, SearchHit, Session, Task, TaskStatus, WorktreeRemoval,
} from './types';

export const api = {
  environment: () => invoke<Environment>('environment'),
  listHistory: () => invoke<Session[]>('list_history'),
  readTranscript: (sessionId: string) => invoke<Entry[]>('read_transcript', { sessionId }),
  searchHistory: (query: string) => invoke<SearchHit[]>('search_history', { query }),
  stats: (windowDays: number, projectRoot: string | null) =>
    invoke<Overview>('stats', {
      windowDays,
      utcOffsetMinutes: -new Date().getTimezoneOffset(),
      projectRoot,
    }),
  listTasks: () => invoke<Task[]>('list_tasks'),
  createTask: (title: string, projectPath: string | null) => invoke<Task>('create_task', { title, projectPath }),
  updateTask: (id: string, patch: { title?: string; notes?: string; status?: TaskStatus }) =>
    invoke<Task>('update_task', { id, ...patch }),
  deleteTask: (id: string) => invoke<void>('delete_task', { id }),
  listAgents: () => invoke<Agent[]>('list_agents'),
  spawnAgent: (projectPath: string, resumeSessionId: string | null, cols: number, rows: number) =>
    invoke<Agent>('spawn_agent', { projectPath, resumeSessionId, cols, rows }),
  writeAgent: (id: string, data: string) => invoke<void>('write_agent', { id, data }),
  resizeAgent: (id: string, cols: number, rows: number) => invoke<void>('resize_agent', { id, cols, rows }),
  killAgent: (id: string) => invoke<void>('kill_agent', { id }),
  restartAgent: (id: string, cols: number, rows: number) => invoke<Agent>('restart_agent', { id, cols, rows }),
  markAgentSeen: (id: string) => invoke<void>('mark_agent_seen', { id }),
  renameAgent: (id: string, name: string) => invoke<void>('rename_agent', { id, name }),
  forgetAgent: (id: string) => invoke<void>('forget_agent', { id }),
  workingChanges: (id: string) => invoke<FileChange[]>('working_changes', { id }),
  workingDiff: (id: string, path: string) => invoke<string>('working_diff', { id, path }),
  mergeRequests: () => invoke<Forge>('merge_requests'),
  previewMrAction: (target: MrRef, action: MrAction) => invoke<ActionPreview>('preview_mr_action', { target, action }),
  runMrAction: (target: MrRef, action: MrAction) => invoke<string>('run_mr_action', { target, action }),
  previewIssueAction: (target: MrRef, action: IssueAction) =>
    invoke<IssueActionPreview>('preview_issue_action', { target, action }),
  runIssueAction: (target: MrRef, action: IssueAction) => invoke<string>('run_issue_action', { target, action }),
  previewIssueCreation: (issue: NewIssue) => invoke<string>('preview_issue_creation', { issue }),
  createIssue: (issue: NewIssue) => invoke<MrRef>('create_issue', { issue }),
  auditLog: () => invoke<AuditEntry[]>('audit_log'),
  prepareIssueStart: (projectRoot: string, issue: MrRef) =>
    invoke<IssueStart>('prepare_issue_start', { projectRoot, issue }),
  startIssueAgent: (projectRoot: string, issue: MrRef, branch: string, base: string, prompt: string) =>
    invoke<{ agentId: string; fetched: boolean }>('start_issue_agent', {
      projectRoot, issue, branch, base, prompt, cols: 120, rows: 36,
    }),
  prepareWorktreeRemoval: (path: string) => invoke<WorktreeRemoval>('prepare_worktree_removal', { path }),
  removeWorktree: (path: string) => invoke<void>('remove_worktree', { path }),
  issues: (project: string | null) => invoke<Issue[]>('issues', { project }),
  mergeRequestStates: (urls: string[]) => invoke<Record<string, MrState>>('merge_request_states', { urls }),
  mergeRequestOutcomes: (refs: MrRef[]) => invoke<MrOutcomes>('merge_request_outcomes', { refs }),
  containers: () => invoke<Container[]>('containers'),
  prepareContainerAction: (id: string, action: ContainerAction) =>
    invoke<ContainerActionPreview>('prepare_container_action', { id, action }),
  runContainerAction: (id: string, action: ContainerAction) => invoke<void>('run_container_action', { id, action }),
  followContainerLogs: (id: string) => invoke<number>('follow_container_logs', { id }),
  stopContainerLogs: (stream: number) => invoke<void>('stop_container_logs', { stream }),
  readAsset: (id: string, path: string) => invoke<AssetContent>('read_asset', { id, path }),
  quit: () => invoke<void>('quit'),
};
