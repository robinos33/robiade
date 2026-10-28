<script lang="ts">
  import { untrack } from 'svelte';
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import type { Container, ContainerAction, Worktree } from '../lib/types';
  import ContainerActionConfirm from '../components/ContainerActionConfirm.svelte';
  import ContainerLogs from '../components/ContainerLogs.svelte';
  import WorktreeCreateDialog from '../components/WorktreeCreateDialog.svelte';
  import WorktreeRemoveDialog from '../components/WorktreeRemoveDialog.svelte';

  let containers = $state<Container[] | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let selectedId = $state<string | null>(null);
  let pending = $state<{ container: Container; action: ContainerAction } | null>(null);
  let creating = $state(false);
  let removing = $state<Worktree | null>(null);

  // Les worktrees, eux, ne sont relus qu'à l'ouverture et sur demande : un `git status` par worktree.
  $effect(() => void untrack(() => app.refreshWorktrees()));

  const liveIn = (path: string) => app.liveAgents.filter((a) => a.worktreePath === path);

  function changesLabel(w: Worktree): string {
    const n = w.git?.changes;
    if (n === null || n === undefined) return '';
    return n === 0 ? 'propre' : `${n} fichier${n > 1 ? 's' : ''} modifié${n > 1 ? 's' : ''}`;
  }

  async function refresh() {
    if (loading) return;
    loading = true;
    try {
      containers = await api.containers();
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // Les conteneurs changent d'état sans moi : relus tant que la vue est ouverte. `untrack` : sans
  // lui, l'effet dépendrait de `loading` et se relancerait à chaque lecture.
  $effect(() => {
    untrack(() => void refresh());
    const timer = setInterval(() => void refresh(), 5_000);
    return () => clearInterval(timer);
  });

  /** Conteneurs du projet ou du worktree sélectionné ; en vue globale, tous, Compose ou non. */
  const visible = $derived(
    (containers ?? []).filter((c) => {
      if (app.project === null) return true;
      if (c.workspace?.projectRoot !== app.project) return false;
      return app.worktree === null || c.workspace.worktreePath === app.worktree;
    }),
  );

  /** Un groupe par projet Compose, dans l'ordre où Docker les a rendus (triés côté Rust). */
  const groups = $derived.by(() => {
    const byProject = new Map<string, Container[]>();
    for (const c of visible) {
      const key = c.composeProject ?? '';
      byProject.set(key, [...(byProject.get(key) ?? []), c]);
    }
    // Les conteneurs lancés hors Compose ferment la liste.
    const loose = byProject.get('');
    byProject.delete('');
    return loose ? [...byProject.entries(), ['', loose] as const] : [...byProject.entries()];
  });

  const selected = $derived(visible.find((c) => c.id === selectedId) ?? null);

  const scopeLabel = $derived(
    app.project === null
      ? 'tous les projets'
      : (app.worktrees.find((w) => w.path === app.worktree)?.name ?? app.projects.find((p) => p.root === app.project)?.name),
  );

  function actionsOf(c: Container): { label: string; action: ContainerAction }[] {
    if (c.state === 'running') return [{ label: 'Redémarrer', action: 'restart' }, { label: 'Arrêter', action: 'stop' }];
    if (c.state === 'paused' || c.state === 'restarting') return [{ label: 'Arrêter', action: 'stop' }];
    if (c.state === 'created' || c.state === 'exited') return [{ label: 'Démarrer', action: 'start' }];
    return [];
  }

  function ports(c: Container): string {
    return c.ports.map((p) => (p.public ? `${p.public}→${p.private}` : `${p.private}`) + (p.protocol === 'tcp' ? '' : `/${p.protocol}`)).join(' ');
  }

  /** Où le projet Compose a été lancé, rapporté à Robiade : worktree, projet, ou simple dossier. */
  function origin(c: Container): string {
    if (c.workspace?.worktreeName) return `${c.workspace.projectName} · ${c.workspace.worktreeName}`;
    if (c.workspace?.inRepository) return c.workspace.projectName;
    return c.workingDir ?? '';
  }
</script>

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Environnements</h1>
    <span class="muted">{scopeLabel}</span>
    <span class="spacer"></span>
    {#if app.project !== null}<button class="btn" onclick={() => (creating = true)}>Nouveau worktree</button>{/if}
    <button class="btn" onclick={() => void Promise.all([refresh(), app.refreshWorktrees()])} disabled={loading}>
      {loading ? 'Lecture…' : 'Relire'}
    </button>
  </header>

  <div class="body" class:split={selected !== null}>
    <div class="scroll">
      {#if app.worktrees.length > 0}
        <div class="block">
          <h2 class="section-title">Worktrees</h2>
          <ul class="list">
            {#each app.worktrees as w (w.path)}
              {@const live = liveIn(w.path)}
              <li class="row" class:active={w.path === app.worktree}>
                <div class="main" title={w.path}>
                  <span class="dot" class:running={live.length > 0} title="{live.length} agent(s) actif(s)"></span>
                  <span class="name ellipsis">{w.name}</span>
                  {#if app.project === null}<span class="tag">{w.projectName}</span>{/if}
                  <span class="mono muted ellipsis branch">{w.git ? (w.git.branch ?? 'tête détachée') : ''}</span>
                  {#if w.git?.locked}<span class="tag">verrouillé</span>{/if}
                  <span class="muted status ellipsis" class:dirty={!!w.git?.changes}>{changesLabel(w)}</span>
                </div>
                <div class="actions">
                  <button class="btn small" onclick={() => void app.spawn(w.path)}>Agent</button>
                  <button class="btn small" onclick={() => (removing = w)}>Supprimer…</button>
                </div>
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      {#if error}
        <div class="error"><strong>Docker illisible.</strong> <span>{error}</span></div>
      {/if}

      {#if containers === null}
        {#if !error}<p class="muted">Lecture de Docker…</p>{/if}
      {:else if groups.length === 0}
        <p class="muted">
          {app.project === null ? 'Aucun conteneur.' : 'Aucun conteneur lancé par Docker Compose depuis ce projet.'}
        </p>
      {:else}
        {#each groups as [project, list] (project)}
          {@const first = list[0]}
          <div class="block">
            <h2 class="section-title" title={first?.workingDir ?? ''}>
              {project || 'Hors Compose'}
              {#if project && first && origin(first) !== project}<span class="origin">{origin(first)}</span>{/if}
            </h2>
            <ul class="list">
              {#each list as c (c.id)}
                <li class="row" class:active={c.id === selectedId}>
                  <button class="main" onclick={() => (selectedId = c.id === selectedId ? null : c.id)} title="Voir les logs">
                    <span class="dot {c.state}" title={c.state}></span>
                    <span class="name ellipsis">{c.name}</span>
                    {#if c.service}<span class="tag">{c.service}</span>{/if}
                    <span class="mono muted ellipsis ports">{ports(c)}</span>
                    <span class="muted status ellipsis">{c.status}</span>
                  </button>
                  <div class="actions">
                    {#each actionsOf(c) as a (a.action)}
                      <button class="btn small" onclick={() => (pending = { container: c, action: a.action })}>{a.label}</button>
                    {/each}
                  </div>
                </li>
              {/each}
            </ul>
          </div>
        {/each}
      {/if}
    </div>

    {#if selected}
      <aside class="side">
        <header class="side-head">
          <strong class="ellipsis">{selected.name}</strong>
          <span class="mono muted ellipsis">{selected.image}</span>
          <span class="spacer"></span>
          <button class="btn ghost small" onclick={() => (selectedId = null)}>Fermer</button>
        </header>
        {#key selected.id}<ContainerLogs id={selected.id} />{/key}
      </aside>
    {/if}
  </div>
</section>

{#if pending}
  <ContainerActionConfirm
    container={pending.container}
    action={pending.action}
    onclose={() => (pending = null)}
    ondone={() => void refresh()}
  />
{/if}

{#if creating && app.project !== null}
  <WorktreeCreateDialog repository={app.project} onclose={() => (creating = false)} />
{/if}
{#if removing}<WorktreeRemoveDialog worktree={removing} onclose={() => (removing = null)} />{/if}

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .body { flex: 1; min-height: 0; display: grid; grid-template-columns: 1fr; }
  .body.split { grid-template-columns: minmax(420px, 1fr) minmax(420px, 1.2fr); }
  .scroll { overflow-y: auto; padding: 22px 24px 40px; display: flex; flex-direction: column; gap: 24px; max-width: 1100px; }
  .section-title { display: flex; align-items: baseline; gap: 10px; }
  .origin { font-weight: 400; text-transform: none; letter-spacing: 0; color: var(--ink-3); }
  .list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
  .row { display: flex; align-items: center; gap: 8px; border-radius: var(--radius-sm); border-bottom: 1px solid var(--line); }
  .row:hover, .row.active { background: var(--bg-hover); }
  .main {
    flex: 1; min-width: 0; display: flex; align-items: center; gap: 12px; height: 36px; padding: 0 10px;
    border: 0; background: none; text-align: left;
  }
  .name { font-weight: 550; flex: none; max-width: 40%; }
  .ports, .branch { flex: none; max-width: 30%; }
  .dirty { color: var(--attention); }
  .status { flex: 1; min-width: 0; text-align: right; font-size: 12px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tag { flex: none; font-size: 11px; padding: 0 6px; border-radius: 8px; border: 1px solid var(--line-strong); color: var(--ink-2); }
  .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; background: var(--ink-3); }
  .dot.running { background: var(--ok); }
  .dot.paused, .dot.restarting { background: var(--attention); }
  .dot.dead { background: var(--danger); }
  .actions { display: flex; justify-content: flex-end; gap: 4px; flex: none; width: 190px; padding-right: 8px; opacity: 0; }
  .row:hover .actions, .row:focus-within .actions { opacity: 1; }
  .small { height: 24px; padding: 0 8px; font-size: 12px; }
  .side { display: flex; flex-direction: column; min-height: 0; padding: 12px 16px 16px; gap: 8px; border-left: 1px solid var(--line); }
  .side-head { display: flex; align-items: baseline; gap: 10px; min-width: 0; }
  .spacer { flex: 1; }
  .error {
    display: flex; flex-direction: column; gap: 4px; padding: 12px 16px;
    border-left: 3px solid var(--danger); border-radius: var(--radius); background: var(--danger-soft);
  }
</style>
