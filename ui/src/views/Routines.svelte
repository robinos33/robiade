<script lang="ts">
  import { untrack } from 'svelte';
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { ago, dateTime, shortPath } from '../lib/format';
  import type { Routine, RoutineAction, RoutineLogs } from '../lib/types';
  import ResizeHandle from '../components/ResizeHandle.svelte';
  import RoutineActionConfirm from '../components/RoutineActionConfirm.svelte';
  import RoutineEditor from '../components/RoutineEditor.svelte';

  let loading = $state(false);
  let selectedLabel = $state<string | null>(null);
  let pending = $state<{ routine: Routine; action: RoutineAction } | null>(null);
  /** Routine ouverte dans l'éditeur ; `'new'` pour en créer une. */
  let editing = $state<Routine | 'new' | null>(null);
  let sideWidth = $state<number | null>(null);
  let bodyW = $state(0);

  async function refresh() {
    if (loading) return;
    loading = true;
    try {
      await app.refreshRoutines();
    } finally {
      loading = false;
    }
  }

  // Ouverte, la vue suit les passages de près : une routine lancée à la main finit en quelques secondes.
  $effect(() => {
    untrack(() => void refresh());
    const timer = setInterval(() => void refresh(), 10_000);
    return () => clearInterval(timer);
  });

  const home = $derived(app.env?.home ?? '');
  const selected = $derived(app.routines.find((r) => r.label === selectedLabel) ?? null);

  const STATES: Record<Routine['state'], string> = {
    running: 'en cours',
    idle: 'active',
    disabled: 'désactivée',
    unloaded: 'non chargée',
  };

  function lastRun(r: Routine): string {
    const e = r.lastExit;
    if (!e) return r.state === 'disabled' ? 'désactivée' : '—';
    if (e.kind === 'never') return 'jamais passée';
    const outcome = e.kind === 'signal' ? (e.signal ?? 'signal') : e.code === 0 ? 'OK' : `code ${e.code}`;
    return r.logsWrittenAt ? `${outcome}, ${ago(r.logsWrittenAt, app.now)}` : outcome;
  }

  function actionsOf(r: Routine): { label: string; action: RoutineAction }[] {
    if (r.state === 'running') return [];
    if (r.state === 'idle') return [{ label: 'Lancer', action: 'run' }, { label: 'Désactiver', action: 'disable' }];
    return [{ label: 'Activer', action: 'enable' }];
  }

  let logs = $state<RoutineLogs | null>(null);
  let logsError = $state<string | null>(null);

  async function readLogs(label: string) {
    try {
      const read = await api.routineLogs(label);
      if (label === selectedLabel) {
        logs = read;
        logsError = null;
      }
    } catch (e) {
      if (label === selectedLabel) logsError = String(e);
    }
  }

  // Journaux relus à la sélection, et à chaque relecture de la routine : un passage a pu les remplir.
  $effect(() => {
    const label = selectedLabel;
    void app.routines;
    if (label === null) return;
    untrack(() => void readLogs(label));
  });

  function select(r: Routine) {
    if (r.label === selectedLabel) {
      selectedLabel = null;
      return;
    }
    logs = null;
    logsError = null;
    selectedLabel = r.label;
  }

  function reveal(r: Routine, script: boolean) {
    api.revealRoutine(r.label, script).catch((e) => app.notify(String(e)));
  }
</script>

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Routines</h1>
    <span class="muted mono">{app.routinePrefix ?? ''}</span>
    <span class="spacer"></span>
    <button class="btn" onclick={() => (editing = 'new')} disabled={app.routinePrefix === null}>Nouvelle routine</button>
    <button class="btn" onclick={() => void refresh()} disabled={loading}>{loading ? 'Lecture…' : 'Relire'}</button>
  </header>

  <div class="body" class:split={selected !== null} bind:clientWidth={bodyW} style:--side-w={sideWidth ? `${sideWidth}px` : null}>
    <div class="scroll">
      <p class="muted small intro">
        Les LaunchAgents de <code>~/Library/LaunchAgents</code> dont le label commence par
        <code>{app.routinePrefix ?? '…'}</code> (préfixe réglé dans Réglages). launchd les lance,
        même Robiade fermé ; Robiade les montre et les pilote.
      </p>

      {#if app.routinesError}
        <div class="error"><strong>launchd illisible.</strong> <span>{app.routinesError}</span></div>
      {/if}

      {#if app.routinePrefix === null}
        {#if !app.routinesError}<p class="muted">Lecture de launchd…</p>{/if}
      {:else if app.routines.length === 0}
        <p class="muted">Aucune routine sous ce préfixe. Change-le dans Réglages pour retrouver les tiennes, ou crée-en une.</p>
      {:else}
        <ul class="list">
          {#each app.routines as r (r.label)}
            <li class="row" class:active={r.label === selectedLabel}>
              <button class="main" onclick={() => select(r)} title={r.label}>
                <span class="dot {r.state}" class:failed={r.failed} title={STATES[r.state]}></span>
                <span class="name ellipsis">{r.name}</span>
                <span class="schedule ellipsis">{r.problem ? 'plist illisible' : r.schedule}</span>
                <span class="next muted ellipsis" title={r.nextRun ? dateTime(r.nextRun) : ''}>
                  {#if r.state === 'running'}{r.pid ? `en cours · pid ${r.pid}` : 'en cours'}
                  {:else if r.nextRun}{ago(r.nextRun, app.now)}
                  {:else}{STATES[r.state]}{/if}
                </span>
                <span class="last ellipsis" class:failed={r.failed}>{lastRun(r)}</span>
              </button>
              <div class="actions">
                {#each actionsOf(r) as a (a.action)}
                  <button class="btn small" onclick={() => (pending = { routine: r, action: a.action })}>{a.label}</button>
                {/each}
                {#if r.form && r.state !== 'running'}
                  <button class="btn small" onclick={() => (editing = r)}>Modifier</button>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if selected}
      {@const r = selected}
      <aside class="side">
        <ResizeHandle id="routines.side" edge="left" min={320} max={bodyW ? bodyW - 420 : Infinity} label="Largeur du détail de la routine" bind:width={sideWidth} />
        <header class="side-head">
          <strong class="ellipsis">{r.name}</strong>
          <span class="muted">{[STATES[r.state], r.runs !== null ? `${r.runs} passage${r.runs > 1 ? 's' : ''}` : ''].filter(Boolean).join(' · ')}</span>
          <span class="spacer"></span>
          <button class="btn ghost small" onclick={() => (selectedLabel = null)}>Fermer</button>
        </header>
        <div class="grid">
          <span class="k">Planning</span>
          <span>{[r.schedule ?? '—', r.nextRun ? `prochain passage ${dateTime(r.nextRun)}` : ''].filter(Boolean).join(' · ')}</span>
          <span class="k">Dernier</span>
          <span><span class:failed={r.failed}>{lastRun(r)}</span>{#if r.logsWrittenAt}<span class="muted">{' '}(heure de la dernière écriture des journaux)</span>{/if}</span>
          {#if r.skill}
            <span class="k">Skill</span>
            <span class="mono path">{r.skill}</span>
          {/if}
          <span class="k">Commande</span>
          <span class="mono path">{r.command.join(' ') || '—'}</span>
          {#if r.workingDir}
            <span class="k">Dossier</span>
            <span class="mono path">{shortPath(r.workingDir, home)}</span>
          {/if}
          <span class="k">Plist</span>
          <span class="mono path">{shortPath(r.file, home)}</span>
        </div>
        {#if r.problem}<p class="error-text small">{r.problem}</p>{/if}
        {#if r.readOnly && !r.problem}<p class="muted small">Lecture seule ici : {r.readOnly}.</p>{/if}
        <div class="buttons">
          <button class="btn small" onclick={() => reveal(r, false)}>Montrer le plist</button>
          {#if r.command.length > 0 && !r.skill}<button class="btn small" onclick={() => reveal(r, true)}>Montrer le script</button>{/if}
          <span class="spacer"></span>
          <button class="btn small danger-ghost" disabled={r.state === 'running'} onclick={() => (pending = { routine: r, action: 'delete' })}>Supprimer…</button>
        </div>
        {#if logsError}<p class="error-text small">{logsError}</p>{/if}
        <div class="logs">
          {#each [['Sortie', logs?.stdout, r.stdout], ['Erreurs', logs?.stderr, r.stderr]] as const as [title, text, path] (title)}
            {#if path && (title === 'Sortie' || r.stderr !== r.stdout)}
              <h3 class="section-title" title={path}>{title} <span class="mono muted">{shortPath(path, home)}</span></h3>
              <pre class="mono">{text ?? (logs ? 'Journal vide ou absent.' : 'Lecture…')}</pre>
            {/if}
          {/each}
          {#if !r.stdout && !r.stderr}<p class="muted small">Aucun journal déclaré dans le plist.</p>{/if}
        </div>
      </aside>
    {/if}
  </div>
</section>

{#if pending}
  <RoutineActionConfirm
    routine={pending.routine}
    action={pending.action}
    onclose={() => (pending = null)}
    ondone={() => {
      if (pending?.action === 'delete' && pending.routine.label === selectedLabel) selectedLabel = null;
      void refresh();
    }}
  />
{/if}
{#if editing}
  <RoutineEditor
    routine={editing === 'new' ? null : editing}
    onclose={() => (editing = null)}
    ondone={(label) => {
      selectedLabel = label;
      void refresh();
    }}
  />
{/if}

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .body { flex: 1; min-height: 0; display: grid; grid-template-columns: 1fr; }
  .body.split { grid-template-columns: minmax(420px, 1fr) var(--side-w, minmax(420px, 1.1fr)); }
  .scroll { overflow-y: auto; padding: 22px 24px 40px; display: flex; flex-direction: column; gap: 16px; max-width: 1100px; }
  .intro { margin: 0; }
  .small { font-size: 12.5px; }
  code { font-family: var(--mono); font-size: 12px; }
  .list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
  .row { display: flex; align-items: center; gap: 8px; border-radius: var(--radius-sm); border-bottom: 1px solid var(--line); }
  .row:hover, .row.active { background: var(--bg-hover); }
  .main {
    flex: 1; min-width: 0; display: grid; grid-template-columns: 8px minmax(120px, 1fr) minmax(0, 1.2fr) minmax(0, 0.8fr) minmax(0, 1fr);
    align-items: center; gap: 12px; height: 36px; padding: 0 10px; border: 0; background: none; text-align: left;
  }
  .name { font-weight: 550; }
  /* Détail ouvert : la liste garde le nom et le dernier passage, le reste est dans le détail. */
  .body.split .main { grid-template-columns: 8px minmax(0, 1fr) minmax(0, 1fr); }
  .body.split .schedule, .body.split .next { display: none; }
  .body.split .actions { width: auto; }
  .schedule { color: var(--ink-2); }
  .last { font-size: 12px; color: var(--ink-2); text-align: right; }
  .next { font-size: 12px; }
  .failed { color: var(--danger); }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--ink-3); }
  .dot.idle { background: var(--ok); }
  .dot.running { background: var(--accent); }
  .dot.disabled, .dot.unloaded { background: transparent; border: 1px solid var(--ink-3); }
  .dot.failed { background: var(--danger); border: 0; }
  .actions { display: flex; justify-content: flex-end; gap: 4px; flex: none; width: 200px; padding-right: 8px; opacity: 0; }
  .row:hover .actions, .row:focus-within .actions { opacity: 1; }
  .btn.small { height: 24px; padding: 0 8px; font-size: 12px; }
  .side { position: relative; display: flex; flex-direction: column; min-height: 0; padding: 12px 16px 16px; gap: 10px; border-left: 1px solid var(--line); }
  .side-head { display: flex; align-items: baseline; gap: 10px; min-width: 0; }
  .spacer { flex: 1; }
  .grid { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; align-items: baseline; font-size: 13px; }
  .k { color: var(--ink-3); font-size: 12.5px; }
  .path { font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; }
  .buttons { display: flex; gap: 6px; align-items: center; }
  .danger-ghost { color: var(--danger); }
  .logs { flex: 1; min-height: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 6px; }
  .logs h3 { margin: 6px 0 0; display: flex; gap: 8px; align-items: baseline; }
  .logs h3 .mono { font-weight: 400; text-transform: none; letter-spacing: 0; font-size: 11.5px; }
  pre {
    margin: 0; padding: 10px 12px; border-radius: var(--radius-sm); background: var(--bg-sunken, var(--bg-hover));
    font-size: 11.5px; line-height: 1.45; white-space: pre-wrap; overflow-wrap: anywhere; max-height: 40vh; overflow-y: auto;
  }
  .error {
    display: flex; flex-direction: column; gap: 4px; padding: 12px 16px;
    border-left: 3px solid var(--danger); border-radius: var(--radius); background: var(--danger-soft);
  }
</style>
