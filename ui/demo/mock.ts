/**
 * Faux backend Tauri pour les captures du README : l'interface réelle, des données inventées.
 * Rien de ce fichier n'entre dans le build (seul `index.html` est une entrée Vite).
 */
import type {
  Agent, Container, DbCell, DbColumn, DbConnection, DbPageRequest, FileChange, Forge, GitWorktree, Issue, MergeRequest, Overview, Session, Task, Tokens, WorktreeRemoval,
} from '../src/lib/types';

const NOW = Date.now();
const MIN = 60_000;
const HOME = '/Users/demo';
const GITLAB = 'https://gitlab.example.com';

const PROJECTS = [
  { name: 'boutique', gitlab: 'shop/boutique' },
  { name: 'facturation', gitlab: 'shop/facturation' },
  { name: 'catalogue-api', gitlab: 'shop/catalogue-api' },
  { name: 'site-vitrine', gitlab: 'web/site-vitrine' },
];

const workspace = (project: number, worktree: string | null = null) => {
  const p = PROJECTS[project]!;
  const root = `${HOME}/code/${p.name}`;
  return {
    projectRoot: root,
    projectName: p.name,
    inRepository: true,
    worktreePath: worktree ? `${HOME}/code/wt-${worktree}` : null,
    worktreeName: worktree ? `wt-${worktree}` : null,
    gitlabProject: p.gitlab,
  };
};

const tokens = (k: number): Tokens => {
  const input = k * 40, output = k * 120, cacheRead = k * 9000, cacheCreation = k * 700;
  return { input, output, cacheRead, cacheCreation, total: input + output + cacheRead + cacheCreation };
};

const gauge = (percent: number) => ({ percent, level: percent > 80 ? 'critical' : percent > 60 ? 'watch' : 'comfortable' }) as const;

function agent(id: string, project: number, worktree: string | null, status: Agent['status'], over: Partial<Agent>): Agent {
  const ws = workspace(project, worktree);
  return {
    ...ws,
    id,
    projectPath: ws.worktreePath ?? ws.projectRoot,
    status,
    needsAttention: status === 'awaitingPermission' || status === 'awaitingInput',
    startedAt: NOW - 42 * MIN,
    lastActivityAt: NOW - MIN,
    waitedMs: 4 * MIN,
    permissionMs: MIN,
    toolCalls: 37,
    tools: [['Read', 14], ['Edit', 9], ['Bash', 8], ['Grep', 6]],
    subagents: 1,
    filesModified: 6,
    lastTool: 'Bash',
    permissionMode: 'acceptEdits',
    permissive: false,
    gauges: {
      model: 'Opus 5.5',
      context: gauge(34),
      contextTokens: 68_000,
      contextWindow: 200_000,
      fiveHour: gauge(41),
      fiveHourResetsAt: NOW + 95 * MIN,
      week: gauge(22),
    },
    assets: [],
    resumedFrom: null,
    name: null,
    ...over,
  };
}

export const AGENT_IDS = ["7f3c9e21-4b8a-4d6e-9c1f-2a5b8e0d3f47", "b2e4a7c9-1d3f-4e8b-a6c2-9f0e5d7b1a38", "c9d1f3e5-7a2b-4c8d-b0e6-3f5a9c1d7e24", "e5a7c9b1-3d5f-4a2c-8e0b-6d1f4a9c2b57"];

const agents: Agent[] = [
  agent('7f3c9e21-4b8a-4d6e-9c1f-2a5b8e0d3f47', 0, 'panier-promo', 'running', { name: 'Codes promo au panier', lastTool: 'Edit' }),
  agent('b2e4a7c9-1d3f-4e8b-a6c2-9f0e5d7b1a38', 1, null, 'awaitingPermission', { name: 'Export comptable CSV', lastTool: 'Bash', permissionMs: 3 * MIN }),
  agent('c9d1f3e5-7a2b-4c8d-b0e6-3f5a9c1d7e24', 2, 'pagination', 'awaitingInput', { name: 'Pagination par curseur', toolCalls: 58 }),
  agent('e5a7c9b1-3d5f-4a2c-8e0b-6d1f4a9c2b57', 3, null, 'idle', { name: 'Page mentions légales', permissionMode: 'auto', permissive: true }),
];

const TITLES: [number, string, string | null, string | null][] = [
  [0, 'Codes promo au panier : un seul code par commande', 'feat/panier-promo', 'shop/boutique/-/merge_requests/412'],
  [1, 'Export comptable CSV, séparateur et encodage configurables', 'feat/export-csv', null],
  [2, 'Pagination par curseur sur /products', 'feat/pagination', 'shop/catalogue-api/-/merge_requests/88'],
  [3, 'Page mentions légales et lien dans le pied de page', 'main', null],
  [0, 'Le total TTC est faux quand on retire un article du panier', 'fix/total-ttc', 'shop/boutique/-/merge_requests/409'],
  [2, 'Pourquoi le cache des catégories ne s’invalide pas ?', 'main', null],
  [1, 'Relance automatique des factures impayées à J+15', 'feat/relances', 'shop/facturation/-/merge_requests/57'],
  [0, 'Migrer les tests du checkout vers Playwright', 'chore/playwright', null],
  [3, 'Optimiser les images de la page d’accueil', 'perf/images', 'web/site-vitrine/-/merge_requests/23'],
  [2, 'Rate limiting par clé d’API', 'feat/rate-limit', null],
  [1, 'Numérotation des avoirs sans trou', 'fix/avoirs', 'shop/facturation/-/merge_requests/55'],
  [0, 'Revue de la MR des adresses de livraison', 'feat/adresses', null],
  [2, 'Ajouter le filtre par marque', 'feat/filtre-marque', null],
  [3, 'Formulaire de contact : protection anti-spam', 'feat/contact', null],
  [0, 'Panier abandonné : e-mail de rappel', 'feat/rappel-panier', null],
];

const sessions: Session[] = TITLES.map(([project, title, branch, mr], i) => {
  const ws = workspace(project);
  const last = NOW - (i < 4 ? i * 3 + 1 : i * i * 47) * MIN;
  return {
    ...ws,
    sessionId: i < 4 ? AGENT_IDS[i]! : `s${i}`,
    projectPath: ws.projectRoot,
    title,
    gitBranch: branch,
    startedAt: last - 50 * MIN,
    lastActivityAt: last,
    userMessages: 3 + ((i * 7) % 19),
    assistantMessages: 20 + ((i * 13) % 60),
    tokens: tokens(30 + i * 11),
    cost: { dollars: 0.8 + ((i * 37) % 90) / 10, unpricedTokens: 0 },
    toolCalls: 12 + ((i * 17) % 70),
    models: ['claude-opus-5-5'],
    mergeRequestUrl: mr ? `${GITLAB}/${mr}` : null,
    headless: false,
  };
});

function mr(i: number, project: number, title: string, over: Partial<MergeRequest>): MergeRequest {
  const p = PROJECTS[project]!;
  return {
    project: p.gitlab,
    iid: i,
    title,
    author: 'demo',
    state: 'opened',
    sourceBranch: 'feat/x',
    targetBranch: 'main',
    webUrl: `${GITLAB}/${p.gitlab}/-/merge_requests/${i}`,
    updatedAt: NOW - 30 * MIN,
    sha: 'a1b2c3d',
    hasConflicts: false,
    unresolvedDiscussions: false,
    mergeStatus: 'mergeable',
    pipeline: 'success',
    wagon: false,
    approvals: { required: 1, left: 0, by: ['camille'] },
    ...over,
  };
}

const forge: Forge = {
  mine: [
    mr(412, 0, 'feat(panier): un seul code promo par commande', { sourceBranch: 'feat/panier-promo', pipeline: 'running', mergeStatus: 'ci_still_running', approvals: { required: 1, left: 1, by: [] } }),
    mr(88, 2, 'feat(api): pagination par curseur sur /products', { sourceBranch: 'feat/pagination', unresolvedDiscussions: true, mergeStatus: 'discussions_not_resolved' }),
    mr(57, 1, 'feat(relances): relance des factures impayées à J+15', { sourceBranch: 'feat/relances', pipeline: 'failed', mergeStatus: 'ci_must_pass', updatedAt: NOW - 3 * 60 * MIN }),
    mr(23, 3, 'perf(accueil): images en AVIF et chargement différé', { sourceBranch: 'perf/images', updatedAt: NOW - 26 * 60 * MIN }),
  ],
  reviews: [
    mr(415, 0, 'feat(livraison): carnet d’adresses du client', { author: 'camille', sourceBranch: 'feat/adresses', pipeline: null, approvals: null, mergeStatus: 'not_approved' }),
    mr(91, 2, 'fix(cache): invalider les catégories à la publication', { author: 'yanis', sourceBranch: 'fix/cache-categories', pipeline: null, approvals: null, mergeStatus: 'not_approved', updatedAt: NOW - 5 * 60 * MIN }),
  ],
};

const issues: Issue[] = [
  { project: 'shop/boutique', iid: 230, state: 'opened', title: 'Panier abandonné : e-mail de rappel après 24 h', labels: ['feature'], assignees: ['demo'], webUrl: `${GITLAB}/shop/boutique/-/issues/230`, updatedAt: NOW - 2 * 60 * MIN, dueDate: null, mergeRequests: 0 },
  { project: 'shop/catalogue-api', iid: 41, state: 'opened', title: 'Filtre par marque sur la recherche', labels: ['feature', 'api'], assignees: ['demo'], webUrl: `${GITLAB}/shop/catalogue-api/-/issues/41`, updatedAt: NOW - 20 * 60 * MIN, dueDate: '2026-11-06', mergeRequests: 0 },
  { project: 'shop/facturation', iid: 19, state: 'opened', title: 'Les avoirs doivent avoir une numérotation continue', labels: ['bug'], assignees: ['demo'], webUrl: `${GITLAB}/shop/facturation/-/issues/19`, updatedAt: NOW - 50 * 60 * MIN, dueDate: null, mergeRequests: 1 },
];

const tasks: Task[] = [
  { id: 't1', title: 'Relire la MR de Camille avant la démo', notes: '', projectPath: null, status: 'doing', createdAt: NOW - 90 * MIN, doneAt: null },
  { id: 't2', title: 'Mettre à jour la doc de l’API de pagination', notes: '', projectPath: `${HOME}/code/catalogue-api`, status: 'todo', createdAt: NOW - 300 * MIN, doneAt: null },
  { id: 't3', title: 'Vider le cache CDN après la mise en prod', notes: '', projectPath: `${HOME}/code/site-vitrine`, status: 'done', createdAt: NOW - 3000 * MIN, doneAt: NOW - 200 * MIN },
];

const DAY = 24 * 60 * MIN;
const overview = (): Overview => {
  const days = Array.from({ length: 30 }, (_, i) => {
    // Numéro de jour depuis l'epoch, comme le renvoie le cœur.
    const day = Math.floor((NOW - (29 - i) * DAY) / DAY);
    const weekend = [3, 4].includes(day % 7); // 1970-01-01 était un jeudi
    const sessions = weekend ? i % 3 : 4 + ((i * 7) % 9);
    return { day, sessions, tokens: sessions * 2_400_000 };
  });
  return {
    sessions: 214,
    tokens: tokens(5800),
    cost: { dollars: 412.6, unpricedTokens: 0 },
    pricesAsOf: '2026-10-01',
    toolCalls: 9_840,
    mergeRequests: 31,
    mergeRequestRefs: [],
    medianDurationMs: 38 * MIN,
    days,
    projects: PROJECTS.map((p, i) => ({
      projectPath: `${HOME}/code/${p.name}`,
      projectName: p.name,
      sessions: [96, 58, 41, 19][i]!,
      tokens: tokens([2600, 1500, 1200, 500][i]!),
      cost: { dollars: [184.2, 106.9, 87.3, 34.2][i]!, unpricedTokens: 0 },
      lastActivityAt: NOW - i * 60 * MIN,
    })),
    models: [
      { model: 'claude-opus-5-5', tokens: tokens(4900), cost: 371.4 },
      { model: 'claude-haiku-5-5', tokens: tokens(900), cost: 41.2 },
    ],
    agents: { agents: 128, failed: 3, waitedMs: 19 * 60 * MIN, permissionMs: 4 * 60 * MIN },
    tools: [['Read', 3120], ['Bash', 2410], ['Edit', 1980], ['Grep', 1260], ['Write', 540], ['Agent', 310], ['WebFetch', 220]],
  };
};

let containerIndex = 1;
const container = (
  name: string,
  project: number | null,
  service: string,
  image: string,
  state: Container['state'],
  status: string,
  ports: [number, number | null][] = [],
): Container => ({
  id: `${(containerIndex++).toString(16).padStart(4, '0')}${'c0ffee'.repeat(10)}`,
  name,
  image,
  state,
  status,
  ports: ports.map(([priv, pub]) => ({ private: priv, public: pub, protocol: 'tcp' })),
  composeProject: project === null ? null : PROJECTS[project]!.name,
  service: project === null ? null : service,
  workingDir: project === null ? null : `${HOME}/code/${PROJECTS[project]!.name}`,
  workspace: project === null ? null : workspace(project),
});

const containers: Container[] = [
  container('boutique-php', 0, 'php', 'boutique/php:8.4', 'running', 'Up 3 hours (healthy)', [[80, null], [443, null]]),
  container('boutique-postgres', 0, 'postgres', 'postgres:17', 'running', 'Up 3 hours', [[5432, 8032]]),
  container('boutique-redis', 0, 'redis', 'redis:7', 'running', 'Up 3 hours', [[6379, 8079]]),
  container('boutique-node', 0, 'node', 'node:22', 'exited', 'Exited (0) 2 hours ago'),
  container('facturation-app', 1, 'app', 'facturation/app:dev', 'running', 'Up 20 minutes', [[8080, 8880]]),
  container('facturation-mariadb', 1, 'mariadb', 'mariadb:11', 'restarting', 'Restarting (1) 4 seconds ago', [[3306, 33018]]),
  container('mailpit', null, '', 'axllent/mailpit', 'exited', 'Exited (0) 3 weeks ago', [[8025, 8025]]),
];

/** Worktrees lus par git : ceux des agents, et un sans session, pour voir la propreté et le verrou. */
const worktree = (project: number, name: string, branch: string, changes: number, locked = false): GitWorktree => ({
  path: `${HOME}/code/wt-${name}`,
  name: `wt-${name}`,
  repository: `${HOME}/code/${PROJECTS[project]!.name}`,
  branch,
  changes,
  locked,
  prunable: false,
});

const worktrees: GitWorktree[] = [
  worktree(0, 'panier-promo', 'ticket-412-codes-promo-au-panier', 3),
  worktree(0, 'livraison', 'ticket-398-frais-de-livraison', 0, true),
  worktree(2, 'pagination', 'ticket-77-pagination-par-curseur', 0),
];

/** Ce qui bloque la suppression d'un worktree de démo : ses fichiers modifiés, s'il en a. */
const BLOCKING: FileChange[] = [
  { path: 'src/Cart/PromoCode.php', kind: 'modified', added: null, removed: null },
  { path: 'tests/Cart/PromoCodePolicyTest.php', kind: 'untracked', added: null, removed: null },
  { path: 'node_modules', kind: 'untracked', added: null, removed: null },
];
const worktreeRemoval = (path: string): WorktreeRemoval => {
  const found = worktrees.find((w) => w.path === path)!;
  const changes = BLOCKING.slice(0, found.changes ?? 0);
  return {
    path,
    repository: found.repository,
    branch: found.branch,
    blocker: changes.length > 0 ? `${changes.length} fichier(s) modifié(s) ou non suivi(s) : commite-les ou annule-les d'abord` : null,
    changes,
    changeCount: changes.length,
  };
};

/** Connexions de démo : une locale, une staging et une production lue dans Vault. */
const dbConnections: DbConnection[] = [
  {
    id: 'db-local', name: 'Boutique locale', engine: 'postgres', stage: 'dev', projectPath: `${HOME}/code/${PROJECTS[0]!.name}`, readOnly: true,
    fields: [
      { part: 'host', source: { kind: 'literal', value: 'localhost' } },
      { part: 'port', source: { kind: 'literal', value: '8032' } },
      { part: 'user', source: { kind: 'literal', value: 'app' } },
      { part: 'password', source: { kind: 'keychain', service: 'pg-boutique-local' } },
    ],
  },
  {
    id: 'db-staging', name: 'Boutique staging', engine: 'postgres', stage: 'staging', projectPath: null, readOnly: true,
    fields: [{ part: 'url', source: { kind: 'vault', mount: 'boutique-kv', path: 'internal/staging/DATABASE', key: 'URL' } }],
  },
  {
    id: 'db-prod', name: 'Boutique prod', engine: 'postgres', stage: 'prod', projectPath: null, readOnly: true,
    fields: [{ part: 'url', source: { kind: 'vault', mount: 'boutique-kv', path: 'internal/prod/DATABASE', key: 'URL' } }],
  },
];

const dbTarget = (id: unknown) => ({
  host: id === 'db-local' ? 'localhost' : `pg-${id === 'db-prod' ? 'prod' : 'staging'}.boutique.internal`,
  port: id === 'db-local' ? 8032 : 5432,
  database: 'boutique',
  user: 'app',
  sslMode: id === 'db-local' ? null : 'require',
});

const dbRelations = [
  { schema: 'public', name: 'customer', kind: 'table', estimatedRows: 12_480 },
  { schema: 'public', name: 'order', kind: 'table', estimatedRows: 1_204_311 },
  { schema: 'public', name: 'order_line', kind: 'table', estimatedRows: 3_918_002 },
  { schema: 'public', name: 'order_summary', kind: 'view', estimatedRows: null },
  { schema: 'public', name: 'promo_code', kind: 'table', estimatedRows: 342 },
  { schema: 'audit', name: 'event', kind: 'table', estimatedRows: 88_120 },
];

const orderColumns: DbColumn[] = [
  { name: 'id', dataType: 'integer', primaryKey: 1 },
  { name: 'reference', dataType: 'character varying(32)', primaryKey: null },
  { name: 'customer_id', dataType: 'integer', primaryKey: null },
  { name: 'status', dataType: 'text', primaryKey: null },
  { name: 'total', dataType: 'numeric(10,2)', primaryKey: null },
  { name: 'payload', dataType: 'jsonb', primaryKey: null },
  { name: 'note', dataType: 'text', primaryKey: null },
  { name: 'created_at', dataType: 'timestamp with time zone', primaryKey: null },
];

const STATUSES = ['paid', 'shipped', 'pending', 'refunded'];

function orderRow(id: number): (DbCell | null)[] {
  const cell = (text: string): DbCell => ({ text, truncated: false });
  const at = new Date(Date.UTC(2026, 9, 28, 9) - id * 37 * MIN).toISOString().replace('T', ' ').replace('.000Z', '+00');
  return [
    cell(String(id)),
    cell(`CMD-2026-${String(id).padStart(6, '0')}`),
    cell(String(1000 + ((id * 7) % 900))),
    cell(STATUSES[id % 4]!),
    cell(((id * 1373) % 50_000 / 100 + 9.9).toFixed(2)),
    cell(JSON.stringify({ promo: id % 5 === 0 ? 'BIENVENUE10' : null, items: (id % 4) + 1, channel: id % 2 ? 'web' : 'app' })),
    id % 3 === 0 ? null : cell(id % 3 === 1 ? '' : 'Livrer en point relais'),
    cell(at),
  ];
}

/** Pages de `public.order` par id décroissant… ou croissant : assez pour voir la grille vivre. */
function dbPage(request: DbPageRequest) {
  const after = request.cursor.kind === 'after' ? Number(request.cursor.values[0]) : 0;
  const ids = Array.from({ length: 200 }, (_, i) => after + i + 1);
  return { columns: orderColumns, rows: ids.map(orderRow), next: { kind: 'after', values: [String(after + 200)] } };
}

const HANDLERS: Record<string, (args: Record<string, unknown>) => unknown> = {
  environment: () => ({ claudePath: '/usr/local/bin/claude', home: HOME }),
  list_history: () => sessions,
  list_agents: () => agents,
  list_tasks: () => tasks,
  merge_requests: () => forge,
  issues: () => issues,
  stats: () => overview(),
  merge_request_states: () => ({}),
  merge_request_outcomes: () => ({ total: 31, merged: 24, opened: 5, closed: 2, unknown: 0 }),
  search_history: () => [],
  read_transcript: () => [],
  audit_log: () => [],
  working_changes: () => [
    { path: 'src/Cart/PromoCode.php', kind: 'modified', added: 42, removed: 7 },
    { path: 'src/Cart/PromoCodePolicy.php', kind: 'added', added: 61, removed: 0 },
    { path: 'tests/Cart/PromoCodePolicyTest.php', kind: 'added', added: 88, removed: 0 },
  ],
  working_diff: () => '',
  containers: () => containers,
  worktrees: (args) => worktrees.filter((w) => w.repository === args.repository),
  prepare_worktree_creation: (args) => ({
    repository: args.repository,
    dir: `${args.repository}/.claude/worktrees/${String(args.branch).replaceAll('/', '-')}`,
    bases: ['origin/staging', 'origin/main'],
    blocker: null,
  }),
  prepare_worktree_removal: (args) => worktreeRemoval(String(args.path)),
  reveal_in_finder: () => null,
  prepare_container_action: (args) => {
    const found = containers.find((c) => c.id === args.id)!;
    return { container: found, action: args.action, blocker: null };
  },
  follow_container_logs: () => 1,
  db_connections: () => dbConnections,
  save_db_connection: (args) => ({ ...(args.connection as DbConnection), id: (args.connection as DbConnection).id ?? 'db-new' }),
  vault_paths: (args) =>
    ['DATABASE', 'MONGO', 'REDIS', 'SENTRY', 'SUPER_MAIL'].map((n) => `${String(args.prefix).replace(/\/?$/, '/')}${n}`),
  vault_keys: (args) => (String(args.path).endsWith('DATABASE') ? ['URL'] : ['DSN', 'URL']),
  resolve_db_target: (args) => dbTarget(args.id),
  test_db_connection: (args) => ({ target: dbTarget(args.id), serverVersion: '17.2', user: 'app', database: 'boutique', readOnly: true }),
  open_db: (args) => ({ database: args.database ?? 'boutique', databases: ['boutique', 'boutique_archive', 'postgres'], relations: dbRelations, readOnly: true }),
  db_page: (args) => dbPage(args.request as DbPageRequest),
};

const callbacks = new Map<number, (payload: unknown) => void>();
let nextId = 1;

/** Les écouteurs `listen('agent:output', …)` de l'app, pour y pousser une sortie de terminal. */
const listeners = new Map<string, number[]>();

export function emit(event: string, payload: unknown) {
  for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id: 0, payload });
}

export function install() {
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      transformCallback(cb: (payload: unknown) => void) {
        const id = nextId++;
        callbacks.set(id, cb);
        return id;
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      convertFileSrc: (p: string) => p,
      async invoke(cmd: string, args: Record<string, unknown> = {}) {
        if (cmd === 'plugin:event|listen') {
          const event = args.event as string;
          listeners.set(event, [...(listeners.get(event) ?? []), args.handler as number]);
          return args.handler;
        }
        if (cmd.startsWith('plugin:')) return null;
        return HANDLERS[cmd]?.(args) ?? null;
      },
    },
    __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
  });
}
