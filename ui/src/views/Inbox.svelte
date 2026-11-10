<script lang="ts">
  import { untrack } from 'svelte';
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import type { Container } from '../lib/types';
  import { ago, duration } from '../lib/format';
  import StatusBadge from '../components/StatusBadge.svelte';
  import MrBadge from '../components/MrBadge.svelte';
  import ProjectTag from '../components/ProjectTag.svelte';

  const REASONS: Record<string, string> = {
    awaitingPermission: 'attend ta permission pour continuer',
    failed: 's’est arrêté sur une erreur',
    awaitingInput: 'a fini son tour, réponse non lue',
  };

  const running = $derived(app.visibleAgents.filter((a) => a.status === 'running' || a.status === 'starting'));
  const doing = $derived(app.visibleTasks.filter((t) => t.status === 'doing'));
  const recent = $derived(app.visibleHistory.slice(0, 6));

  let containers = $state<Container[]>([]);

  // Docker éteint ou illisible : rien à signaler ici, la vue Environnements dit pourquoi.
  $effect(() => {
    const read = () => api.containers().then((c) => (containers = c), () => (containers = []));
    untrack(() => void read());
    const timer = setInterval(() => void read(), 30_000);
    return () => clearInterval(timer);
  });

  /** En échec : redémarre en boucle, mort, malade, ou arrêté sur un code d'erreur. */
  const failing = (c: Container) =>
    c.state === 'restarting' ||
    c.state === 'dead' ||
    c.status.includes('(unhealthy)') ||
    (c.state === 'exited' && !c.status.startsWith('Exited (0)'));

  const brokenContainers = $derived(
    containers.filter((c) => failing(c) && (app.project === null || c.workspace?.projectRoot === app.project)),
  );
</script>

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Inbox</h1>
    <span class="muted">{app.project ? app.projects.find((p) => p.root === app.project)?.name : 'Tous les projets'}</span>
  </header>

  <div class="scroll">
    <div class="block">
      <h2 class="section-title">Demande ton action</h2>
      {#if app.attention.length === 0}
        <div class="calm">
          <span class="ok-dot"></span>
          <div>
            <strong>Rien ne t'attend.</strong>
            <span class="muted">Les agents qui ont besoin de toi apparaîtront ici, par ordre d'urgence.</span>
          </div>
        </div>
      {:else}
        <ul class="cards">
          {#each app.attention as a (a.id)}
            <li>
              <button class="card {a.status}" onclick={() => app.openAgent(a.id)}>
                <StatusBadge status={a.status} />
                <span class="what"><strong><ProjectTag name={a.projectName} root={a.projectRoot} /></strong> {REASONS[a.status]}</span>
                <span class="muted num">depuis {ago(a.lastActivityAt, app.now).replace('il y a ', '')}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if app.mrAttention.length > 0 || app.pendingReviews.length > 0}
      <div class="block">
        <h2 class="section-title">{app.forgeName}</h2>
        <ul class="cards">
          {#each app.mrAttention as mr (mr.webUrl)}
            <li>
              <button class="card failed" onclick={(e) => app.read({ kind: 'mr', project: mr.project, iid: mr.iid, webUrl: mr.webUrl }, e)}>
                <MrBadge url={mr.webUrl} state={mr.state} />
                <span class="what">
                  <strong>{mr.title}</strong>
                  {[
                    mr.pipeline === 'failed' ? 'pipeline rouge' : '',
                    mr.hasConflicts ? 'conflits' : '',
                    mr.unresolvedDiscussions ? 'discussions ouvertes' : '',
                  ].filter(Boolean).join(', ')}
                </span>
              </button>
            </li>
          {/each}
          {#each app.pendingReviews as mr (mr.webUrl)}
            <li>
              <button class="card awaitingInput" onclick={(e) => app.read({ kind: 'mr', project: mr.project, iid: mr.iid, webUrl: mr.webUrl }, e)}>
                <MrBadge url={mr.webUrl} state={mr.state} />
                <span class="what"><strong>{mr.title}</strong> attend ta review ({mr.author})</span>
                <span class="muted num">{ago(mr.updatedAt, app.now)}</span>
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if app.failingRoutines.length > 0}
      <div class="block">
        <h2 class="section-title">Routines en échec</h2>
        <ul class="cards">
          {#each app.failingRoutines as r (r.label)}
            <li class="with-action">
              <button class="card failed" onclick={() => (app.view = 'routines')}>
                <span class="what">
                  <strong>{r.name}</strong>
                  {r.lastExit?.kind === 'signal' ? `arrêtée par un signal (${r.lastExit.signal})` : `a échoué (code ${r.lastExit?.code})`} à son dernier passage
                </span>
                {#if r.logsWrittenAt}<span class="muted num">{ago(r.logsWrittenAt, app.now)}</span>{/if}
              </button>
              <button class="btn ghost seen" onclick={() => app.markRoutineSeen(r)} title="Retirer de l'Inbox jusqu'au prochain passage">Vu</button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if brokenContainers.length > 0}
      <div class="block">
        <h2 class="section-title">Conteneurs en échec</h2>
        <ul class="cards">
          {#each brokenContainers as c (c.id)}
            <li>
              <button class="card failed" onclick={() => (app.view = 'env')}>
                <span class="what">
                  <strong>{c.name}</strong>
                  <span class="muted">{#if c.workspace}<ProjectTag name={c.workspace.projectName} root={c.workspace.projectRoot} />{:else}{c.composeProject ?? ''}{/if}</span>
                </span>
                <span class="muted num">{c.status}</span>
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    <div class="grid">
      <div class="block">
        <h2 class="section-title">Agents au travail</h2>
        {#if running.length === 0}
          <p class="muted">Aucun agent en cours.</p>
        {:else}
          <ul class="rows">
            {#each running as a (a.id)}
              <li>
                <button class="row" onclick={() => app.openAgent(a.id)}>
                  <StatusBadge status={a.status} compact />
                  <span class="grow"><ProjectTag name={a.projectName} root={a.projectRoot} /></span>
                  <span class="mono muted">{a.lastTool ?? '—'}</span>
                  <span class="muted num">{duration(app.now - a.startedAt)}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <div class="block">
        <h2 class="section-title">Tâches en cours</h2>
        {#if doing.length === 0}
          <p class="muted">Aucune tâche perso en cours.</p>
        {:else}
          <ul class="rows">
            {#each doing as t (t.id)}
              <li><button class="row" onclick={() => (app.view = 'tasks')}><span class="grow">{t.title}</span></button></li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>

    <div class="block">
      <h2 class="section-title">Dernières sessions</h2>
      <ul class="rows">
        {#each recent as s (s.sessionId)}
          <li>
            <button class="row" onclick={() => app.openSession(s.sessionId)}>
              <span class="proj"><ProjectTag name={s.projectName} root={s.projectRoot} /></span>
              <span class="grow ellipsis">{s.title}</span>
              {#if s.mergeRequestUrl}<MrBadge url={s.mergeRequestUrl} />{/if}
              <span class="muted num">{ago(s.lastActivityAt, app.now)}</span>
            </button>
          </li>
        {:else}
          <li class="muted">{app.historyLoaded ? 'Aucune session.' : 'Lecture de l’historique…'}</li>
        {/each}
      </ul>
    </div>

  </div>
</section>

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .scroll { overflow-y: auto; padding: 22px 24px 40px; display: flex; flex-direction: column; gap: 28px; max-width: 1100px; }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 28px; }
  .calm {
    display: flex; align-items: center; gap: 14px; padding: 16px 18px;
    border: 1px dashed var(--line-strong); border-radius: var(--radius);
  }
  .calm strong { display: block; font-weight: 600; }
  .ok-dot { width: 10px; height: 10px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 0 4px color-mix(in srgb, var(--ok) 18%, transparent); }
  ul { list-style: none; margin: 0; padding: 0; }
  .cards { display: flex; flex-direction: column; gap: 8px; }
  .card {
    display: flex; align-items: center; gap: 14px; width: 100%; padding: 12px 16px; text-align: left;
    border: 1px solid var(--line); border-left-width: 3px; border-radius: var(--radius); background: var(--bg-raised);
  }
  .card:hover { background: var(--bg-hover); }
  .card.awaitingPermission { border-left-color: var(--attention); background: var(--attention-soft); }
  .card.failed { border-left-color: var(--danger); }
  .card.awaitingInput { border-left-color: var(--accent); }
  .what { flex: 1; }
  .with-action { display: flex; gap: 6px; align-items: stretch; }
  .seen { flex: none; align-self: center; }
  .rows { display: flex; flex-direction: column; }
  .row {
    display: flex; align-items: center; gap: 12px; width: 100%; height: 34px; padding: 0 10px;
    border: 0; border-radius: var(--radius-sm); background: none; text-align: left;
  }
  .row:hover { background: var(--bg-hover); }
  .grow { flex: 1; min-width: 0; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .proj { width: 130px; flex: none; color: var(--ink-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
