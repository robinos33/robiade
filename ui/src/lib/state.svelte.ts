import { listen } from '@tauri-apps/api/event';
import { api } from './api';
import { writeOutput } from './terminals';
import type { Agent, Environment, Project, Session, Task } from './types';

export type View = 'inbox' | 'agents' | 'history' | 'tasks' | 'stats';

const ATTENTION_ORDER: Record<string, number> = { awaitingPermission: 0, failed: 1, awaitingInput: 2 };

class AppState {
  view = $state<View>('inbox');
  /** Racine du projet sélectionné ; `null` = vue globale. */
  project = $state<string | null>(null);
  env = $state<Environment | null>(null);
  agents = $state<Agent[]>([]);
  history = $state<Session[]>([]);
  historyLoaded = $state(false);
  tasks = $state<Task[]>([]);
  activeAgentId = $state<string | null>(null);
  selectedSessionId = $state<string | null>(null);
  newAgentOpen = $state(false);
  paletteOpen = $state(false);
  /** Agents qu'une fermeture interromprait ; non nul tant que la confirmation est affichée. */
  quitRequest = $state<Agent[] | null>(null);
  toast = $state<string | null>(null);
  now = $state(Date.now());

  projects: Project[] = $derived.by(() => {
    const byRoot = new Map<string, Project>();
    for (const s of this.history) {
      const p = byRoot.get(s.projectRoot) ?? { root: s.projectRoot, name: s.projectName, lastActivityAt: 0, sessions: 0 };
      p.sessions += 1;
      p.lastActivityAt = Math.max(p.lastActivityAt, s.lastActivityAt);
      byRoot.set(s.projectRoot, p);
    }
    for (const a of this.agents) {
      const p = byRoot.get(a.projectRoot) ?? { root: a.projectRoot, name: a.projectName, lastActivityAt: 0, sessions: 0 };
      p.lastActivityAt = Math.max(p.lastActivityAt, a.lastActivityAt);
      byRoot.set(a.projectRoot, p);
    }
    return [...byRoot.values()].sort((a, b) => b.lastActivityAt - a.lastActivityAt);
  });

  inProject = (root: string | null | undefined) => this.project === null || root === this.project;

  visibleAgents = $derived(this.agents.filter((a) => this.inProject(a.projectRoot)));
  attention = $derived(
    this.visibleAgents
      .filter((a) => a.needsAttention)
      .sort((a, b) => (ATTENTION_ORDER[a.status] ?? 9) - (ATTENTION_ORDER[b.status] ?? 9)),
  );
  liveAgents = $derived(this.agents.filter((a) => a.status !== 'ended'));
  visibleHistory = $derived(this.history.filter((s) => this.inProject(s.projectRoot)));
  visibleTasks = $derived(
    this.tasks.filter((t) => this.project === null || t.projectPath === this.project),
  );

  /** Session de l'historique correspondant à un agent (même identifiant, ou session reprise). */
  sessionOf(agent: Agent): Session | undefined {
    const id = agent.resumedFrom ?? agent.id;
    return this.history.find((s) => s.sessionId === id);
  }

  async init() {
    await Promise.all([
      listen<Agent[]>('agents:changed', (e) => (this.agents = e.payload)),
      listen<{ id: string; data: string }>('agent:output', (e) => writeOutput(e.payload.id, e.payload.data)),
      listen<Agent[]>('app:quit-requested', (e) => (this.quitRequest = e.payload)),
    ]);
    this.env = await api.environment();
    this.agents = await api.listAgents();
    this.tasks = await api.listTasks();
    await this.refreshHistory();
    setInterval(() => (this.now = Date.now()), 15_000);
    // Les transcripts des agents vivants grossissent : on relit (le cache ne reparse que les fichiers modifiés).
    setInterval(() => this.liveAgents.length > 0 && void this.refreshHistory(), 20_000);
  }

  async refreshHistory() {
    try {
      this.history = await api.listHistory();
    } catch (e) {
      this.notify(`Historique illisible : ${e}`);
    } finally {
      this.historyLoaded = true;
    }
  }

  async refreshTasks() {
    this.tasks = await api.listTasks();
  }

  async spawn(projectPath: string, resumeSessionId: string | null = null) {
    try {
      const agent = await api.spawnAgent(projectPath, resumeSessionId, 120, 36);
      this.activeAgentId = agent.id;
      this.view = 'agents';
      this.newAgentOpen = false;
    } catch (e) {
      this.notify(String(e));
    }
  }

  openAgent(id: string) {
    this.activeAgentId = id;
    this.view = 'agents';
    void api.markAgentSeen(id);
  }

  openSession(id: string) {
    this.selectedSessionId = id;
    this.view = 'history';
  }

  notify(message: string) {
    this.toast = message;
    setTimeout(() => this.toast === message && (this.toast = null), 5000);
  }
}

export const app = new AppState();
