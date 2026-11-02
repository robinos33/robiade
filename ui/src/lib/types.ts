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
  /** Effort de raisonnement rapporté par les hooks ; absent avant le premier outil. */
  effort: string | null;
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
  /** `AAAA-MM-JJ`. */
  dueOn: string | null;
  /** Issue d'équipe référencée ; son titre et son état se lisent sur GitLab. */
  issue: MrRef | null;
  issueUrl: string | null;
  sessionId: string | null;
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

/** Issue à créer, telle que saisie ; le domaine la valide. */
export interface NewIssue {
  project: string;
  title: string;
  description: string;
  labels: string[];
  assignees: string[];
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
/** Un worktree, connu par les sessions qui y ont tourné ou par git ; `git` est absent tant qu'il n'est pas relu. */
export interface Worktree { path: string; name: string; root: string; projectName: string; lastActivityAt: number; git: GitWorktree | null }

/** Un worktree lié, lu par git ; `changes` est `null` si le dossier est illisible ou disparu. */
export interface GitWorktree {
  path: string;
  name: string;
  repository: string;
  branch: string | null;
  changes: number | null;
  locked: boolean;
  prunable: boolean;
}

/** Ce que « créer un worktree » ferait ; `blocker` dit pourquoi c'est refusé. */
export interface WorktreeCreation { repository: string; dir: string; bases: string[]; blocker: string | null }

/** Ce que « supprimer le worktree » ferait ; `blocker` dit pourquoi c'est refusé. */
/** `changes` : les premiers fichiers que la suppression ferait perdre ; `changeCount` les compte tous. */
export interface WorktreeRemoval {
  path: string;
  repository: string;
  branch: string | null;
  blocker: string | null;
  changes: FileChange[];
  changeCount: number;
}

export type ContainerState = 'created' | 'running' | 'paused' | 'restarting' | 'exited' | 'dead' | 'other';
export type ContainerAction = 'start' | 'stop' | 'restart';

/** Un conteneur Docker ; `workspace` est le dépôt d'où son projet Compose a été lancé. */
export interface Container {
  id: string;
  name: string;
  image: string;
  state: ContainerState;
  /** Résumé de Docker (« Up 3 hours »). */
  status: string;
  ports: { private: number; public: number | null; protocol: string }[];
  composeProject: string | null;
  service: string | null;
  workingDir: string | null;
  workspace: Workspace | null;
}

/** Ce que l'action ferait, sur le conteneur relu ; `blocker` dit pourquoi c'est refusé. */
export interface ContainerActionPreview { container: Container; action: ContainerAction; blocker: string | null }

export type DbEngine = 'postgres' | 'mariadb' | 'mongo' | 'elasticsearch';
export type DbStage = 'dev' | 'staging' | 'prod';
export type DbPart = 'url' | 'host' | 'port' | 'database' | 'user' | 'password';

/** D'où vient la valeur d'un champ. Une valeur n'est portée qu'en `literal`, jamais pour un secret. */
export type DbSource =
  | { kind: 'literal'; value: string }
  | { kind: 'keychain'; service: string }
  | { kind: 'vault'; mount: string; path: string; key: string };

export interface DbField { part: DbPart; source: DbSource }

/** Une connexion ; une production est toujours en lecture seule (règle tenue côté Rust). */
export interface DbConnection {
  id: string;
  name: string;
  engine: DbEngine;
  stage: DbStage;
  projectPath: string | null;
  readOnly: boolean;
  fields: DbField[];
}

export type DbConnectionInput = Omit<DbConnection, 'id'> & { id: string | null };

/** Où mène une connexion, valeurs résolues, sans le mot de passe. */
export interface DbTarget { host: string; port: number; database: string | null; user: string | null; sslMode: string | null }

/** Ce que le serveur a dit à la connexion d'essai ; `readOnly` est lu sur le serveur. */
export interface DbProbe { target: DbTarget; serverVersion: string; user: string; database: string; readOnly: boolean }

export type DbRelationKind = 'table' | 'view' | 'materializedView' | 'foreign';

/** `estimatedRows` : estimation du planificateur, absente si la table n'a jamais été analysée. */
export interface DbRelation { schema: string; name: string; kind: DbRelationKind; estimatedRows: number | null }

/** Une session ouverte ; `readOnly` est lu sur le serveur. */
export interface DbOverview { database: string; databases: string[]; relations: DbRelation[]; readOnly: boolean }

export interface DbColumn { name: string; dataType: string; primaryKey: number | null }

/** Une valeur affichable ; `null` dans une ligne pour NULL, distinct de la chaîne vide. */
export interface DbCell { text: string; truncated: boolean }

/** Opaque : renvoyé tel quel pour demander la page suivante. */
export type DbCursor = { kind: 'start' } | { kind: 'after'; values: string[] } | { kind: 'offset'; offset: number };

export type DbFilter =
  | { column: string; op: 'equals' | 'contains'; value: string }
  | { column: string; op: 'isNull' | 'isNotNull' };

export interface DbSort { column: string; descending: boolean }

export interface DbPageRequest { sort: DbSort | null; filters: DbFilter[]; cursor: DbCursor }

/** `next` absent : dernière page. */
export interface DbPage { columns: DbColumn[]; rows: (DbCell | null)[][]; next: DbCursor | null }

/** Résultat d'une instruction de la console : au plus 1000 lignes, ou le nombre de lignes touchées. */
export interface DbConsoleResult {
  columns: DbColumn[];
  rows: (DbCell | null)[][];
  truncated: boolean;
  affected: number | null;
  elapsedMs: number;
}

/**
 * Une écriture revient d'abord à confirmer, avec l'instruction exacte qui partira ; en prod
 * (`prod`), en tapant le nom de la connexion. Une modification de prod revient ensuite en
 * attente, avec son nombre de lignes, à valider ou annuler avant `delayMs`.
 */
export type DbConsoleReply =
  | { kind: 'confirm'; statement: string; connection: string; prod: boolean; target: string }
  | { kind: 'done'; statement: string; result: DbConsoleResult }
  | { kind: 'pending'; statement: string; affected: number; delayMs: number };

/** `sessionClosed` : la session n'était plus en lecture seule après coup, elle est fermée. */
export interface DbFinishedWrite { committed: boolean; affected: number; sessionClosed: boolean }

export interface DbConsoleHistory { sql: string; at: number }
