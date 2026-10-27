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
  /** Du premier au dernier message ; `null` sans session sur la période. */
  medianDurationMs: number | null;
  days: { day: number; sessions: number; tokens: number }[];
  projects: { projectPath: string; projectName: string; sessions: number; tokens: Tokens; cost: Cost; lastActivityAt: number }[];
  models: { model: string; tokens: Tokens; cost: number | null }[];
  /** Tiré des hooks : seulement les agents lancés dans Robiade. */
  agents: { agents: number; failed: number; waitedMs: number; permissionMs: number };
  tools: [string, number][];
}

export interface Environment { claudePath: string | null; home: string }

export interface Project { root: string; name: string; lastActivityAt: number; sessions: number }
export interface Worktree { path: string; name: string; root: string; projectName: string; lastActivityAt: number }
