export type AgentStatus =
  | 'starting' | 'running' | 'awaitingPermission' | 'awaitingInput' | 'idle' | 'failed' | 'ended';

export interface Agent {
  id: string;
  projectPath: string;
  projectRoot: string;
  projectName: string;
  status: AgentStatus;
  needsAttention: boolean;
  startedAt: number;
  lastActivityAt: number;
  waitedMs: number;
  toolCalls: number;
  lastTool: string | null;
  permissionMode: string | null;
  permissive: boolean;
  resumedFrom: string | null;
}

export interface Tokens { input: number; output: number; cacheRead: number; cacheCreation: number; total: number }

export interface Session {
  sessionId: string;
  projectPath: string;
  projectRoot: string;
  projectName: string;
  title: string;
  gitBranch: string | null;
  startedAt: number;
  lastActivityAt: number;
  userMessages: number;
  assistantMessages: number;
  tokens: Tokens;
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
  toolCalls: number;
  mergeRequests: number;
  days: { day: number; sessions: number; tokens: number }[];
  projects: { projectPath: string; projectName: string; sessions: number; tokens: Tokens; lastActivityAt: number }[];
  tools: [string, number][];
}

export interface Environment { claudePath: string | null; home: string }

export interface Project { root: string; name: string; lastActivityAt: number; sessions: number }
