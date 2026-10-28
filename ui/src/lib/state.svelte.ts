import { listen } from '@tauri-apps/api/event';
import { api } from './api';
import { writeOutput } from './terminals';
import type { Agent, Environment, Forge, MergeRequest, MrState, Project, Session, Task, Workspace, Worktree } from './types';

export type View = 'inbox' | 'agents' | 'history' | 'mr' | 'tasks' | 'stats';

const ATTENTION_ORDER: Record<string, number> = { awaitingPermission: 0, failed: 1, awaitingInput: 2 };

class AppState {
  view = $state<View>('inbox');
  /** Racine du projet sélectionné ; `null` = vue globale. */
  project = $state<string | null>(null);
  /** Worktree sélectionné, dans le projet sélectionné ; `null` = tout le projet. */
  worktree = $state<string | null>(null);
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
  /** MR lues sur GitLab ; `null` avant la première lecture. */
  forge = $state<Forge | null>(null);
  forgeError = $state<string | null>(null);
  forgeLoading = $state(false);
  /** État GitLab des MR liées aux sessions, par URL ; absente = pas encore lue ou illisible. */
  mrStates = $state<Record<string, MrState>>({});

  /** Dépôts principaux seulement : ni worktree, ni sous-dossier, ni dossier disparu. */
  projects: Project[] = $derived.by(() => {
    const byRoot = new Map<string, Project>();
    for (const s of this.history.filter((s) => s.inRepository)) {
      const p = byRoot.get(s.projectRoot) ?? this.newProject(s);
      p.sessions += 1;
      p.lastActivityAt = Math.max(p.lastActivityAt, s.lastActivityAt);
      byRoot.set(s.projectRoot, p);
    }
    for (const a of this.agents.filter((a) => a.inRepository)) {
      const p = byRoot.get(a.projectRoot) ?? this.newProject(a);
      p.lastActivityAt = Math.max(p.lastActivityAt, a.lastActivityAt);
      byRoot.set(a.projectRoot, p);
    }
    return [...byRoot.values()].sort((a, b) => b.lastActivityAt - a.lastActivityAt);
  });

  private newProject(w: Workspace): Project {
    return { root: w.projectRoot, name: w.projectName, lastActivityAt: 0, sessions: 0, gitlabProject: w.gitlabProject };
  }

  /** Projet GitLab du projet sélectionné ; `null` en vue globale ou pour un dépôt hors de l'instance. */
  selectedGitlabProject = $derived(this.projects.find((p) => p.root === this.project)?.gitlabProject ?? null);

  /** Vrai si la MR appartient au projet sélectionné (GitLab ignore la casse des chemins). */
  inGitlabScope = (mr: MergeRequest) =>
    this.project === null || mr.project.toLowerCase() === this.selectedGitlabProject?.toLowerCase();

  /** Worktrees encore présents, du projet sélectionné (ou de tous), les plus récents d'abord. */
  worktrees: Worktree[] = $derived.by(() => {
    const byPath = new Map<string, Worktree>();
    for (const w of [...this.history, ...this.agents]) {
      if (!w.worktreePath || !this.inProject(w.projectRoot)) continue;
      const at = 'lastActivityAt' in w ? w.lastActivityAt : 0;
      const known = byPath.get(w.worktreePath);
      if (known) known.lastActivityAt = Math.max(known.lastActivityAt, at);
      else
        byPath.set(w.worktreePath, {
          path: w.worktreePath,
          name: w.worktreeName ?? w.worktreePath,
          root: w.projectRoot,
          projectName: w.projectName,
          lastActivityAt: at,
        });
    }
    return [...byPath.values()].sort((a, b) => b.lastActivityAt - a.lastActivityAt);
  });

  inProject = (root: string | null | undefined) => this.project === null || root === this.project;
  inScope = (w: Workspace) => this.inProject(w.projectRoot) && (this.worktree === null || w.worktreePath === this.worktree);

  visibleAgents = $derived(this.agents.filter((a) => this.inScope(a)));
  attention = $derived(
    this.visibleAgents
      .filter((a) => a.needsAttention)
      .sort((a, b) => (ATTENTION_ORDER[a.status] ?? 9) - (ATTENTION_ORDER[b.status] ?? 9)),
  );
  liveAgents = $derived(this.agents.filter((a) => a.status !== 'ended'));
  visibleHistory = $derived(this.history.filter((s) => this.inScope(s)));
  visibleTasks = $derived(
    this.tasks.filter((t) => this.project === null || t.projectPath === this.project),
  );

  /** Mes MR qui demandent une action : pipeline rouge, conflits, discussions ouvertes. */
  mrAttention: MergeRequest[] = $derived(
    (this.forge?.mine ?? [])
      .filter((m) => m.pipeline === 'failed' || m.hasConflicts || m.unresolvedDiscussions)
      .filter((m) => this.inGitlabScope(m)),
  );
  /** Reviews qui m'attendent, dans le projet sélectionné. */
  pendingReviews: MergeRequest[] = $derived((this.forge?.reviews ?? []).filter((m) => this.inGitlabScope(m)));

  /** Sessions liées à une MR par leur `pr-link`. */
  sessionsOf(mr: MergeRequest): Session[] {
    return this.history.filter((s) => s.mergeRequestUrl === mr.webUrl);
  }

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
    void this.refreshForge();
    // Pipelines et reviews bougent sans moi : relus toutes les 2 minutes.
    setInterval(() => void this.refreshForge(), 120_000);
  }

  /** Lit l'état des MR des sessions récentes ; le cache côté Rust évite de relire les MR closes. */
  async refreshMrStates() {
    const urls = [...new Set(this.history.slice(0, 200).flatMap((s) => (s.mergeRequestUrl ? [s.mergeRequestUrl] : [])))];
    if (urls.length === 0) return;
    try {
      this.mrStates = { ...this.mrStates, ...(await api.mergeRequestStates(urls)) };
    } catch {
      // Token absent ou refusé : la vue Merge requests le dit déjà, les badges restent neutres.
    }
  }

  async refreshForge() {
    if (this.forgeLoading) return;
    this.forgeLoading = true;
    try {
      this.forge = await api.mergeRequests();
      const fresh = Object.fromEntries([...this.forge.mine, ...this.forge.reviews].map((m) => [m.webUrl, m.state]));
      this.mrStates = { ...this.mrStates, ...fresh };
      this.forgeError = null;
    } catch (e) {
      this.forgeError = String(e);
    } finally {
      this.forgeLoading = false;
    }
  }

  async refreshHistory() {
    try {
      this.history = await api.listHistory();
      void this.refreshMrStates();
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

  /** Filtre sur le projet ; s'il a des agents, ouvre le premier de la liste (vivant de préférence). */
  selectProject(root: string | null) {
    this.project = root;
    this.worktree = null;
    if (root !== null) this.openFirstAgent((a) => a.projectRoot === root);
  }

  /** Comme pour un projet : filtre sur le worktree et ouvre son premier agent s'il en a un. */
  selectWorktree(w: Worktree) {
    this.project = w.root;
    this.worktree = w.path;
    this.openFirstAgent((a) => a.worktreePath === w.path);
  }

  private openFirstAgent(match: (a: Agent) => boolean) {
    const agents = this.agents.filter(match);
    const first = agents.find((a) => a.status !== 'ended') ?? agents[0];
    if (first) this.openAgent(first.id);
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
