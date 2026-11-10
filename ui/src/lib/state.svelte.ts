import { forgeName as nameOf, mrPrefix as prefixOf } from './forge';
import { listen } from '@tauri-apps/api/event';
import { api } from './api';
import { localDay } from './format';
import { moveInOrder, orderProjects, storedOrder, storeOrder } from './projectOrder';
import { writeOutput } from './terminals';
import type { Agent, Environment, Forge, GitWorktree, MergeRequest, MrState, Project, ProjectColors, QuitRequest, ReadingTarget, Routine, Session, Shortcut, Task, Update, Workspace, Worktree } from './types';

export type View = 'inbox' | 'agents' | 'history' | 'mr' | 'tasks' | 'stats' | 'env' | 'db' | 'code' | 'routines' | 'settings';

/**
 * Échecs de routine déjà vus : par label, le nombre de passages au moment où je l'ai marqué vu. Un
 * nouveau passage en échec change ce nombre, et la routine revient dans l'Inbox. Préférence du poste.
 */
const SEEN_KEY = 'robiade.routines.seen';
function storedSeen(): Record<string, number> {
  try {
    const raw = JSON.parse(localStorage.getItem(SEEN_KEY) ?? '{}');
    return raw && typeof raw === 'object' ? raw : {};
  } catch {
    return {};
  }
}

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
  /** « Aller au fichier » (⌘P) de l'onglet Code. */
  fileFinderOpen = $state(false);
  /** Fenêtre « Reprendre une session », l'équivalent visuel de `/resume`. */
  resumeOpen = $state(false);
  /** Agents qu'une fermeture, ou une relance, interromprait ; non nul tant que la confirmation est affichée. */
  quitRequest = $state<QuitRequest | null>(null);
  /** Mise à jour au lancement, lue au chargement puis suivie par `app:update`. */
  update = $state<Update | null>(null);
  toast = $state<string | null>(null);
  now = $state(Date.now());
  /** MR lues sur GitLab ; `null` avant la première lecture. */
  forge = $state<Forge | null>(null);
  forgeError = $state<string | null>(null);
  forgeLoading = $state(false);
  /** État GitLab des MR liées aux sessions, par URL ; absente = pas encore lue ou illisible. */
  mrStates = $state<Record<string, MrState>>({});
  /** Issue ou MR affichée dans le panneau de lecture ; `null` : panneau fermé. */
  reading = $state<ReadingTarget | null>(null);
  /** Worktrees lus par git, par dépôt ; un dépôt absent n'a pas encore été relu. */
  gitWorktrees = $state<Record<string, GitWorktree[]>>({});
  /** Couleur de projet par racine de dépôt principal ; absente = aucune. Une identité, jamais un état. */
  projectColors = $state<Record<string, string>>({});
  /** Teintes proposées dans Réglages, dans l'ordre. */
  palette = $state<string[]>([]);
  /** Raccourcis de la barre sous le terminal, dans l'ordre ; réglés dans Réglages. */
  shortcuts = $state<Shortcut[]>([]);
  /** Routines du préfixe réglé, relues sur launchd ; `routinePrefix` est `null` avant la première lecture. */
  routines = $state<Routine[]>([]);
  routinePrefix = $state<string | null>(null);
  routinesError = $state<string | null>(null);
  seenRoutineFailures = $state<Record<string, number>>(storedSeen());
  /** En échec à leur dernier passage, et pas encore vues. */
  failingRoutines = $derived(
    this.routines.filter((r) => r.failed && this.seenRoutineFailures[r.label] !== r.runs),
  );

  /** Ordre des projets rangés à la main dans la barre latérale ; voir `projectOrder.ts`. */
  projectOrder = $state<string[]>(storedOrder());

  /**
   * Dépôts principaux seulement : ni worktree, ni sous-dossier, ni dossier disparu. Ceux qui ont un
   * agent vivant d'abord, puis l'ordre rangé, puis la dernière activité.
   */
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
    const recent = [...byRoot.values()].sort((a, b) => b.lastActivityAt - a.lastActivityAt);
    const live = new Set(this.liveAgents.map((a) => a.projectRoot));
    return orderProjects(recent, this.projectOrder, (root) => live.has(root));
  });

  /** Range `root` juste avant `before`, ou après `after` en fin de son groupe. */
  moveProject(root: string, target: { before: string } | { after: string }) {
    const next = moveInOrder(this.projectOrder, this.projects.map((p) => p.root), root, target);
    if (next === this.projectOrder) return;
    this.projectOrder = next;
    storeOrder(next);
  }

  private newProject(w: Workspace): Project {
    return { root: w.projectRoot, name: w.projectName, lastActivityAt: 0, sessions: 0, gitlabProject: w.gitlabProject };
  }

  /** Projet GitLab du projet sélectionné ; `null` en vue globale ou pour un dépôt hors de l'instance. */
  /** Nom de la forge en vigueur, pour les textes ; « la forge » sans réglage. */
  forgeName = $derived(nameOf(this.env?.forge?.provider));
  /** Préfixe du numéro d'une MR : `!` sur GitLab, `#` sur GitHub. */
  mrPrefix = $derived(prefixOf(this.env?.forge?.provider));
  selectedGitlabProject = $derived(this.projects.find((p) => p.root === this.project)?.gitlabProject ?? null);

  /** Vrai si la MR appartient au projet sélectionné (GitLab ignore la casse des chemins). */
  inGitlabScope = (mr: MergeRequest) =>
    this.project === null || mr.project.toLowerCase() === this.selectedGitlabProject?.toLowerCase();

  /**
   * Worktrees encore présents, du projet sélectionné (ou de tous), les plus récents d'abord : ceux
   * où une session a tourné, et ceux que git connaît sans session, en dernier.
   */
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
          git: null,
        });
    }
    for (const [root, list] of Object.entries(this.gitWorktrees)) {
      if (!this.inProject(root)) continue;
      const projectName = this.projects.find((p) => p.root === root)?.name ?? root;
      for (const g of list.filter((g) => !g.prunable)) {
        const known = byPath.get(g.path);
        if (known) known.git = g;
        else byPath.set(g.path, { path: g.path, name: g.name, root, projectName, lastActivityAt: 0, git: g });
      }
    }
    return [...byPath.values()].sort((a, b) => b.lastActivityAt - a.lastActivityAt);
  });

  /** Teinte du projet de cette racine ; `null` sans couleur. */
  colorOf = (root: string | null | undefined): string | null => (root ? this.projectColors[root] ?? null : null);
  /** Teinte du dépôt local qui porte ce projet de forge (GitLab ignore la casse des chemins). */
  colorOfForge = (project: string): string | null => {
    const lower = project.toLowerCase();
    return this.colorOf(this.projects.find((p) => p.gitlabProject?.toLowerCase() === lower)?.root);
  };
  /** Variable CSS de la teinte, pour un liseré ou une icône ; `null` sans couleur. */
  colorVar = (root: string | null | undefined): string | null => {
    const color = this.colorOf(root);
    return color ? `var(--project-${color})` : null;
  };

  showColors(c: ProjectColors) {
    this.palette = c.palette;
    this.projectColors = c.colors;
  }

  inProject = (root: string | null | undefined) => this.project === null || root === this.project;
  inScope = (w: Workspace) => this.inProject(w.projectRoot) && (this.worktree === null || w.worktreePath === this.worktree);

  visibleAgents = $derived(this.agents.filter((a) => this.inScope(a)));
  attention = $derived(
    this.visibleAgents
      .filter((a) => a.needsAttention)
      .sort((a, b) => (ATTENTION_ORDER[a.status] ?? 9) - (ATTENTION_ORDER[b.status] ?? 9)),
  );
  liveAgents = $derived(this.agents.filter((a) => a.status !== 'ended'));
  /** Les sessions qu'on a menées soi-même ; les headless ne servent qu'aux stats. */
  conversations = $derived(this.history.filter((s) => !s.headless));
  visibleHistory = $derived(this.conversations.filter((s) => this.inScope(s)));
  visibleTasks = $derived(
    this.tasks.filter((t) => this.project === null || t.projectPath === this.project),
  );
  today = $derived(localDay(this.now));
  /** Pas encore faite, et son échéance est aujourd'hui ou déjà passée. */
  isDue = (t: Task) => t.status !== 'done' && t.dueOn !== null && t.dueOn <= this.today;
  dueTasks = $derived(this.visibleTasks.filter(this.isDue));
  /** Tous projets confondus, tâches globales comprises : de quoi signaler une échéance hors du projet affiché. */
  allDueTasks = $derived(this.tasks.filter(this.isDue));
  /**
   * Signal le plus fort, pour un bouton qui cache les listes : l'accent compte les agents qui
   * attendent et les échéances tombées, de tous les projets ; sinon, le bleu compte les agents qui
   * tournent. Un agent qui attend ne disparaît pas avec la liste qui le montrait.
   */
  signal = $derived.by((): { hot: boolean; count: number; text: string } | null => {
    const waiting = this.agents.filter((a) => a.needsAttention).length;
    const due = this.allDueTasks.length;
    if (waiting + due > 0) {
      const parts = [waiting && `${waiting} agent(s) en attente`, due && `${due} échéance(s) tombée(s)`];
      return { hot: true, count: waiting + due, text: parts.filter(Boolean).join(', ') };
    }
    const live = this.liveAgents.length;
    return live ? { hot: false, count: live, text: `${live} agent(s) actif(s)` } : null;
  });

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

  /** Session de l'historique correspondant à la session en cours d'un agent. */
  sessionOf(agent: Agent): Session | undefined {
    const id = agent.sessionId;
    return this.history.find((s) => s.sessionId === id);
  }

  /** Écouteurs et minuteries posés par `init`, à libérer si le module est remplacé. */
  private teardown: (() => void)[] = [];

  /**
   * MR de la session d'un agent : son `pr-link` (écrit pour `gh`, pas pour `glab`), sinon la MR
   * ouverte du même projet GitLab dont la branche source est celle de la session. Titre et état ne
   * sont connus que de cette dernière.
   */
  mergeRequestOf(agent: Agent): { url: string; state?: MrState; title?: string } | null {
    const session = this.sessionOf(agent);
    if (session?.mergeRequestUrl) return { url: session.mergeRequestUrl };
    const project = agent.gitlabProject?.toLowerCase();
    const branch = session?.gitBranch;
    if (!project || !branch) return null;
    const mr = [...(this.forge?.mine ?? []), ...(this.forge?.reviews ?? [])].find(
      (m) => m.project.toLowerCase() === project && m.sourceBranch === branch,
    );
    return mr ? { url: mr.webUrl, state: mr.state, title: mr.title } : null;
  }

  async init() {
    this.teardown.push(
      ...(await Promise.all([
        listen<Agent[]>('agents:changed', (e) => (this.agents = e.payload)),
        listen<{ id: string; data: string }>('agent:output', (e) => writeOutput(e.payload.id, e.payload.data)),
        listen<QuitRequest>('app:quit-requested', (e) => (this.quitRequest = e.payload)),
        listen<Update>('app:update', (e) => (this.update = e.payload)),
        // Clic sur une notification macOS : l'agent qui m'attend.
        listen<string>('agent:open', (e) => this.openAgent(e.payload)),
      ])),
    );
    // Un événement arrivé pendant la lecture est plus récent qu'elle : il garde la main.
    void api.updateStatus().then((u) => (this.update ??= u));
    void api.projectColors().then((c) => this.showColors(c), () => {});
    void api.shortcuts().then((s) => (this.shortcuts = s), () => {});
    this.env = await api.environment();
    this.agents = await api.listAgents();
    this.tasks = await api.listTasks();
    await this.refreshHistory();
    void this.refreshWorktrees();
    this.every(15_000, () => (this.now = Date.now()));
    // Les transcripts des agents vivants grossissent : on relit (le cache ne reparse que les fichiers modifiés).
    this.every(20_000, () => this.liveAgents.length > 0 && void this.refreshHistory());
    void this.refreshForge();
    // Pipelines et reviews bougent sans moi : relus toutes les 2 minutes.
    this.every(120_000, () => void this.refreshForge());
    // Une routine passe à heure fixe, au plus une fois par minute : 5 minutes suffisent à l'Inbox.
    void this.refreshRoutines();
    this.every(300_000, () => void this.refreshRoutines());
  }

  private every(ms: number, run: () => void) {
    const timer = setInterval(run, ms);
    this.teardown.push(() => clearInterval(timer));
  }

  dispose() {
    this.teardown.splice(0).forEach((stop) => stop());
  }

  /** Lit l'état des MR des sessions récentes ; le cache côté Rust évite de relire les MR closes. */
  async refreshMrStates() {
    const urls = [...new Set(this.history.slice(0, 200).flatMap((s) => (s.mergeRequestUrl ? [s.mergeRequestUrl] : [])))];
    if (urls.length === 0 || !this.env?.forge) return;
    try {
      this.mrStates = { ...this.mrStates, ...(await api.mergeRequestStates(urls)) };
    } catch {
      // Token absent ou refusé : la vue Merge requests le dit déjà, les badges restent neutres.
    }
  }

  async refreshForge() {
    if (this.forgeLoading || !this.env?.forge) return;
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

  /** Relit les worktrees du projet sélectionné, ou de tous ; un dépôt illisible garde sa dernière lecture. */
  async refreshWorktrees() {
    const roots = this.project === null ? this.projects.map((p) => p.root) : [this.project];
    const read = await Promise.all(
      roots.map((root) => api.worktrees(root).then((list) => [root, list] as const, () => null)),
    );
    this.gitWorktrees = { ...this.gitWorktrees, ...Object.fromEntries(read.filter((r) => r !== null)) };
  }

  async refreshRoutines() {
    try {
      const read = await api.routines();
      this.routines = read.routines;
      this.routinePrefix = read.prefix;
      this.routinesError = null;
    } catch (e) {
      this.routinesError = String(e);
    }
  }

  /** Retire l'échec de l'Inbox jusqu'au prochain passage ; un succès l'en sort de toute façon. */
  markRoutineSeen(r: Routine) {
    const seen = { ...this.seenRoutineFailures, [r.label]: r.runs ?? 0 };
    for (const label of Object.keys(seen)) if (!this.routines.some((x) => x.label === label)) delete seen[label];
    this.seenRoutineFailures = seen;
    try {
      localStorage.setItem(SEEN_KEY, JSON.stringify(seen));
    } catch {
      // Stockage inaccessible : vu pour la session.
    }
  }

  async refreshTasks() {
    this.tasks = await api.listTasks();
  }

  /** Dans le worktree ou le projet sélectionné ; sans sélection, on demande où. */
  newAgent() {
    const target = this.worktree ?? this.project;
    if (target === null) this.newAgentOpen = true;
    else void this.spawn(target);
  }

  async spawn(projectPath: string, resumeSessionId: string | null = null) {
    try {
      const agent = await api.spawnAgent(projectPath, resumeSessionId, 120, 36);
      this.activeAgentId = agent.id;
      this.view = 'agents';
      this.newAgentOpen = false;
      // Worktree supprimé depuis : la session repart dans le dépôt, sur la branche qui y est active.
      const worktrees = `${agent.projectPath}/.claude/worktrees/`;
      if (resumeSessionId !== null && projectPath.startsWith(worktrees)) {
        const worktree = projectPath.slice(worktrees.length);
        this.notify(`Worktree « ${worktree} » supprimé : session reprise dans ${agent.projectName}, sur la branche qui y est active.`);
      }
    } catch (e) {
      this.notify(String(e));
    }
  }

  /** Filtre sur le projet ; s'il a des agents, ouvre le premier de la liste (vivant de préférence). */
  selectProject(root: string | null) {
    this.project = root;
    this.worktree = null;
    void this.refreshWorktrees();
    if (root !== null) this.openFirstAgent((a) => a.projectRoot === root);
  }

  /** Racine que l'onglet Code parcourt : le worktree sélectionné, sinon le projet. */
  get codeRoot(): string | null {
    return this.worktree ?? this.project;
  }

  /** ⌘P, depuis n'importe quelle vue : l'onglet Code, et sa recherche s'il a un projet. */
  findFile() {
    this.view = 'code';
    this.fileFinderOpen = this.codeRoot !== null;
  }

  /** Comme pour un projet : filtre sur le worktree et ouvre son premier agent s'il en a un. */
  selectWorktree(w: Worktree) {
    this.project = w.root;
    this.worktree = w.path;
    this.openFirstAgent((a) => a.worktreePath === w.path);
  }

  /** L'onglet Code suit le projet choisi sans céder la place à son agent. */
  private openFirstAgent(match: (a: Agent) => boolean) {
    if (this.view === 'code') return;
    const agents = this.agents.filter(match);
    const first = agents.find((a) => a.status !== 'ended') ?? agents[0];
    if (first) this.openAgent(first.id);
  }

  openAgent(id: string) {
    this.activeAgentId = id;
    this.view = 'agents';
    void api.markAgentSeen(id);
  }

  /** Ouvre une issue ou une MR dans le panneau de lecture ; avec ⌘, dans le navigateur, comme avant. */
  read(target: ReadingTarget, e?: MouseEvent) {
    if (e?.metaKey) this.openUrl(target.webUrl);
    else this.reading = target;
  }

  /** Page de la forge dans le navigateur ; un refus (autre hôte) s'affiche. */
  openUrl(url: string) {
    api.openForgeUrl(url).catch((e) => this.notify(String(e)));
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

// En dev, Vite réexécute ce module à chaque modification : sans ça, l'ancienne instance garde son
// abonnement à `agent:output` et chaque sortie de terminal est écrite deux fois dans le même xterm.
import.meta.hot?.dispose(() => app.dispose());
