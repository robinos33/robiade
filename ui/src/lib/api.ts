import { invoke } from '@tauri-apps/api/core';
import type {
  Agent, AssetContent, IssueReading, MrReading, ProjectFile, ProjectFiles, Container, ContributionBucket, Granularity, DbConnection, DbConnectionInput, DbConsoleHistory, DbConsoleReply, DbFinishedWrite, DbOverview, DbPage, DbPageRequest, DbProbe, DbTarget, ContainerAction, ContainerActionPreview, Entry, FileChange, Environment, ActionPreview, AuditEntry, Forge, Issue, IssueAction, IssueActionPreview, IssueStart, MrAction, NewIssue, MrOutcomes, MrRef, MrState, Overview, SaveOutcome, SearchHit, Session, Settings, ForgeSettings, VaultSettings, Task, TaskStatus, GitWorktree, Update, WorktreeCreation, WorktreeRemoval,
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
  createTask: (title: string, projectPath: string | null, dueOn: string | null) =>
    invoke<Task>('create_task', { title, projectPath, dueOn }),
  /** `dueOn` à `null` retire l'échéance. */
  scheduleTask: (id: string, dueOn: string | null) => invoke<Task>('schedule_task', { id, dueOn }),
  /** `reference` : `#12`, `groupe/projet#12` ou URL ; `null` retire le lien. */
  linkTaskIssue: (id: string, reference: string | null) => invoke<Task>('link_task_issue', { id, reference }),
  /** `sessionId` à `null` retire le lien. */
  linkTaskSession: (id: string, sessionId: string | null) => invoke<Task>('link_task_session', { id, sessionId }),
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
  readMergeRequest: (target: MrRef) => invoke<MrReading>('read_merge_request', { target }),
  readIssue: (target: MrRef) => invoke<IssueReading>('read_issue', { target }),
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
  prepareTaskStart: (projectRoot: string, id: string) => invoke<IssueStart>('prepare_task_start', { projectRoot, id }),
  startTaskAgent: (projectRoot: string, id: string, branch: string, base: string, prompt: string) =>
    invoke<{ agentId: string; fetched: boolean }>('start_task_agent', {
      projectRoot, id, branch, base, prompt, cols: 120, rows: 36,
    }),
  worktrees: (repository: string) => invoke<GitWorktree[]>('worktrees', { repository }),
  prepareWorktreeCreation: (repository: string, branch: string) =>
    invoke<WorktreeCreation>('prepare_worktree_creation', { repository, branch }),
  createWorktree: (repository: string, branch: string, base: string) =>
    invoke<boolean>('create_worktree', { repository, branch, base }),
  prepareWorktreeRemoval: (path: string) => invoke<WorktreeRemoval>('prepare_worktree_removal', { path }),
  /** `discard` : modifications non commitées que j'accepte de perdre, 0 pour un arbre propre. */
  removeWorktree: (path: string, discard: number) => invoke<void>('remove_worktree', { path, discard }),
  /** `path` relatif au worktree ; vide pour montrer le worktree lui-même. */
  revealInFinder: (worktree: string, path: string) => invoke<void>('reveal_in_finder', { worktree, path }),
  issues: (project: string | null) => invoke<Issue[]>('issues', { project }),
  mergeRequestStates: (urls: string[]) => invoke<Record<string, MrState>>('merge_request_states', { urls }),
  mergeRequestOutcomes: (refs: MrRef[]) => invoke<MrOutcomes>('merge_request_outcomes', { refs }),
  gitlabActivity: (granularity: Granularity) =>
    invoke<ContributionBucket[]>('gitlab_activity', { granularity, utcOffsetMinutes: -new Date().getTimezoneOffset() }),
  containers: () => invoke<Container[]>('containers'),
  prepareContainerAction: (id: string, action: ContainerAction) =>
    invoke<ContainerActionPreview>('prepare_container_action', { id, action }),
  runContainerAction: (id: string, action: ContainerAction) => invoke<void>('run_container_action', { id, action }),
  followContainerLogs: (id: string) => invoke<number>('follow_container_logs', { id }),
  stopContainerLogs: (stream: number) => invoke<void>('stop_container_logs', { stream }),
  dbConnections: () => invoke<DbConnection[]>('db_connections'),
  saveDbConnection: (connection: DbConnectionInput) => invoke<DbConnection>('save_db_connection', { connection }),
  deleteDbConnection: (id: string) => invoke<void>('delete_db_connection', { id }),
  vaultPaths: (mount: string, prefix: string) => invoke<string[]>('vault_paths', { mount, prefix }),
  vaultKeys: (mount: string, path: string) => invoke<string[]>('vault_keys', { mount, path }),
  resolveDbTarget: (id: string) => invoke<DbTarget>('resolve_db_target', { id }),
  testDbConnection: (id: string) => invoke<DbProbe>('test_db_connection', { id }),
  openDb: (id: string, database: string | null) => invoke<DbOverview>('open_db', { id, database }),
  closeDb: (id: string) => invoke<void>('close_db', { id }),
  dbPage: (id: string, schema: string, name: string, request: DbPageRequest) =>
    invoke<DbPage>('db_page', { id, schema, name, request }),
  /**
   * `cursor` : position dans `text` (unités UTF-16, comme l'éditeur) ; `null` : `text` est une seule instruction.
   * `confirmation` : réponse à une confirmation ; en prod, le nom de la connexion tapé à la main.
   */
  runDbStatement: (id: string, text: string, cursor: number | null, confirmation: string | null) =>
    invoke<DbConsoleReply>('run_db_statement', { id, text, cursor, confirmation }),
  finishDbWrite: (id: string, commit: boolean) => invoke<DbFinishedWrite>('finish_db_write', { id, commit }),
  cancelDbStatement: (id: string) => invoke<void>('cancel_db_statement', { id }),
  dbConsoleHistory: (id: string) => invoke<DbConsoleHistory[]>('db_console_history', { id }),
  readAsset: (id: string, path: string) => invoke<AssetContent>('read_asset', { id, path }),
  /** Fichier d'un lien `file://` du terminal de l'agent, s'il reste dans son dossier de travail. */
  readLinkedFile: (id: string, uri: string) => invoke<ProjectFile>('read_linked_file', { id, uri }),
  /**
   * Chemins cités en clair dans le terminal de l'agent (`src/foo.rs`), dans l'ordre : le chemin
   * absolu d'un fichier que `readLinkedFile` lirait, `null` sinon.
   */
  resolveLinkedPaths: (id: string, cited: string[]) => invoke<(string | null)[]>('resolve_linked_paths', { id, cited }),
  /** Fichiers du projet ou du worktree `root`, hors ignorés par git. */
  projectFiles: (root: string) => invoke<ProjectFiles>('project_files', { root }),
  /** `reveal` ouvre un fichier de secrets connu, qui sinon n'est pas lu. */
  readProjectFile: (root: string, path: string, reveal: boolean) =>
    invoke<ProjectFile>('read_project_file', { root, path, reveal }),
  updateStatus: () => invoke<Update | null>('update_status'),
  /** Relance pour prendre la mise à jour ; des agents vivants passent d'abord par la confirmation. */
  relaunch: () => invoke<void>('relaunch'),
  quit: (relaunch: boolean) => invoke<void>('quit', { relaunch }),
  /** Page de la forge réglée, dans le navigateur ; toute autre URL est refusée par Rust. */
  openForgeUrl: (url: string) => invoke<void>('open_forge_url', { url }),
  settings: () => invoke<Settings>('settings'),
  /** Sans `confirmedHost`, un nouvel hôte n'est pas enregistré : la réponse le donne à confirmer. */
  saveForgeSettings: (forge: ForgeSettings | null, confirmedHost: string | null) =>
    invoke<SaveOutcome>('save_forge_settings', { forge, confirmedHost }),
  saveVaultSettings: (vault: VaultSettings | null, confirmedHost: string | null) =>
    invoke<SaveOutcome>('save_vault_settings', { vault, confirmedHost }),
};
