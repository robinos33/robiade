export type AgentStatus =
  | 'starting' | 'running' | 'awaitingPermission' | 'awaitingInput' | 'idle' | 'failed' | 'ended';

/** Rattachement d'un dossier de travail : dépôt principal, et worktree lié s'il existe encore. */
export interface Workspace {
  projectRoot: string;
  projectName: string;
  /** Le projet est un dépôt Git présent : il a sa place dans la liste des projets. */
  inRepository: boolean;
  worktreePath: string | null;
  worktreeName: string | null;
  /** Projet GitLab du remote `origin`, s'il est sur l'instance configurée. */
  gitlabProject: string | null;
}

export interface Agent extends Workspace {
  id: string;
  projectPath: string;
  status: AgentStatus;
  needsAttention: boolean;
  startedAt: number;
  lastActivityAt: number;
  waitedMs: number;
  /** Part de `waitedMs` passée sur une demande de permission. */
  permissionMs: number;
  toolCalls: number;
  /** Appels par outil, le plus fréquent en tête. */
  tools: [string, number][];
  subagents: number;
  filesModified: number;
  lastTool: string | null;
  permissionMode: string | null;
  permissive: boolean;
  gauges: Gauges;
  /** Fichiers affichables écrits par l'agent, le plus récent en tête. */
  assets: Asset[];
  resumedFrom: string | null;
  /** Nom donné à l'onglet ; l'historique l'affiche aussi à la place du titre. */
  name: string | null;
}

/** Fichier modifié depuis le dernier commit ; `added`/`removed` à `null` pour un binaire. */
export interface FileChange {
  path: string;
  kind: 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked';
  added: number | null;
  removed: number | null;
}

export type AssetKind = 'image' | 'html' | 'markdown' | 'text';
export interface Asset { path: string; name: string; kind: AssetKind; writtenAt: number }
/** Contenu d'un asset : `dataUrl` pour une image, `text` pour le reste. */
export interface AssetContent { kind: AssetKind; text: string | null; dataUrl: string | null }

export interface Gauge { percent: number; level: 'comfortable' | 'watch' | 'critical' }

/** Relevés de la statusline de Claude Code ; tout peut manquer avant le premier rafraîchissement. */
export interface Gauges {
  model: string | null;
  context: Gauge | null;
  contextTokens: number | null;
  contextWindow: number | null;
  fiveHour: Gauge | null;
  fiveHourResetsAt: number | null;
  week: Gauge | null;
}

/** Coût estimé au tarif public de l'API ; `unpricedTokens` : tokens de modèles sans prix connu. */
export interface Cost { dollars: number; unpricedTokens: number }

export interface Tokens { input: number; output: number; cacheRead: number; cacheCreation: number; total: number }

export interface Session extends Workspace {
  sessionId: string;
  projectPath: string;
  title: string;
  gitBranch: string | null;
  startedAt: number;
  lastActivityAt: number;
  userMessages: number;
  assistantMessages: number;
  tokens: Tokens;
  cost: Cost;
  toolCalls: number;
  models: string[];
  mergeRequestUrl: string | null;
  /** Lancée par un script (`claude -p`, Agent SDK) : comptée dans les stats, pas proposée à la reprise. */
  headless: boolean;
}

/** Extrait d'une session trouvée : segments `[texte, trouvé]`, rendus comme texte. */
export interface SearchHit { sessionId: string; snippet: [string, boolean][] }

export interface Entry { kind: 'user' | 'assistant' | 'tool'; at: number; text: string }

export type TaskStatus = 'todo' | 'doing' | 'done';
export interface Task {
  id: string;
  title: string;
  notes: string;
  projectPath: string | null;
  status: TaskStatus;
  createdAt: number;
  doneAt: number | null;
}

export interface Overview {
  sessions: number;
  tokens: Tokens;
  cost: Cost;
  /** Date du relevé de la grille tarifaire. */
  pricesAsOf: string;
  toolCalls: number;
  mergeRequests: number;
  /** MR distinctes des sessions de la période, dont l'état se lit ensuite sur GitLab. */
  mergeRequestRefs: MrRef[];
  /** Du premier au dernier message ; `null` sans session sur la période. */
  medianDurationMs: number | null;
  days: { day: number; sessions: number; tokens: number }[];
  projects: { projectPath: string; projectName: string; sessions: number; tokens: Tokens; cost: Cost; lastActivityAt: number }[];
  models: { model: string; tokens: Tokens; cost: number | null }[];
  /** Tiré des hooks : seulement les agents lancés dans Robiade. */
  agents: { agents: number; failed: number; waitedMs: number; permissionMs: number };
  tools: [string, number][];
}

export interface MrRef { project: string; iid: number }

export type MrState = 'draft' | 'opened' | 'merged' | 'closed';
export type PipelineStatus = 'running' | 'success' | 'failed' | 'manual' | 'canceled';

/** Titre, auteur et branches viennent de GitLab : non fiables, rendus comme texte. */
export interface MergeRequest extends MrRef {
  title: string;
  author: string;
  state: MrState;
  sourceBranch: string;
  targetBranch: string;
  webUrl: string;
  updatedAt: number;
  /** Commit de tête vu à l'affichage. */
  sha: string;
  hasConflicts: boolean;
  unresolvedDiscussions: boolean;
  /** `detailed_merge_status` de GitLab, tel quel. */
  mergeStatus: string;
  /** `null` sans pipeline, ou non lu (MR des autres). */
  pipeline: PipelineStatus | null;
  /** Wagon intermédiaire d'un train : ne cible ni staging ni master. */
  wagon: boolean;
  /** Lues pour mes MR seulement. */
  approvals: { required: number; left: number; by: string[] } | null;
}

/** Tâche d'équipe, lue sur GitLab : titre et labels non fiables, rendus comme texte. */
export interface Issue {
  project: string;
  iid: number;
  title: string;
  state: IssueState;
  labels: string[];
  assignees: string[];
  webUrl: string;
  updatedAt: number;
  /** `AAAA-MM-JJ`. */
  dueDate: string | null;
  mergeRequests: number;
}

export type IssueState = 'opened' | 'closed';

export type IssueAction =
  | { kind: 'close' }
  | { kind: 'reopen' }
  /** Listes complètes voulues ; le domaine en déduit l'écart. */
  | { kind: 'assign'; assignees: string[] }
  | { kind: 'label'; labels: string[] };

/** Issue relue sur GitLab et description de ce qui partira, produite par le domaine. */
export interface IssueActionPreview {
  project: string;
  iid: number;
  title: string;
  state: IssueState;
  assignees: string[];
  labels: string[];
  action: string;
}

export type MrAction =
  | { kind: 'comment'; body: string }
  | { kind: 'approve' }
  | { kind: 'rebase' }
  | { kind: 'retryPipeline' }
  | { kind: 'markReady' }
  | { kind: 'markDraft' }
  | { kind: 'merge'; expectedSha: string };

/** Cible relue sur GitLab et description de ce qui partira, produite par le domaine. */
export interface ActionPreview {
  project: string;
  iid: number;
  title: string;
  sourceBranch: string;
  targetBranch: string;
  author: string;
  action: string;
}

export interface AuditEntry { at: number; target: string; action: string; outcome: string }

/** Proposition pour démarrer un agent sur une issue ; tout est relu avant l'envoi. */
export interface IssueStart {
  repository: string;
  branch: string;
  bases: string[];
  worktreeDir: string;
  prompt: string;
  tabName: string;
}

export interface Forge { mine: MergeRequest[]; reviews: MergeRequest[] }

export interface MrOutcomes { total: number; merged: number; opened: number; closed: number; unknown: number }

export interface Environment { claudePath: string | null; home: string }

export interface Project { root: string; name: string; lastActivityAt: number; sessions: number; gitlabProject: string | null }
export interface Worktree { path: string; name: string; root: string; projectName: string; lastActivityAt: number }

/** Ce que « supprimer le worktree » ferait ; `blocker` dit pourquoi c'est refusé. */
export interface WorktreeRemoval { path: string; repository: string; branch: string | null; blocker: string | null }
