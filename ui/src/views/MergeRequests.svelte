<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { ago } from '../lib/format';
  import type { AuditEntry, MergeRequest, MrAction } from '../lib/types';
  import { MERGE_STATUS, PIPELINE, actionsOf } from '../lib/mergeRequests';
  import MrBadge from '../components/MrBadge.svelte';
  import MrActionConfirm from '../components/MrActionConfirm.svelte';

  /** Action en attente de confirmation. */
  let pending = $state<{ mr: MergeRequest; action: MrAction } | null>(null);
  let audit = $state<AuditEntry[]>([]);
  let showAudit = $state(false);

  $effect(() => {
    void app.forge; // relu après chaque rafraîchissement, donc après chaque action
    if (showAudit) api.auditLog().then((a) => (audit = a), (e) => app.notify(String(e)));
  });

  const act = (mr: MergeRequest, action: MrAction) => (pending = { mr, action });

  const shortProject = (path: string) => path.split('/').slice(-1)[0];

  $effect(() => {
    if (app.forge === null) void app.refreshForge();
  });

  const reviews = $derived((app.forge?.reviews ?? []).filter(app.inGitlabScope));
  const mine = $derived((app.forge?.mine ?? []).filter(app.inGitlabScope));
  const scopeLabel = $derived(
    app.project === null ? 'tous les projets' : (app.selectedGitlabProject ?? `projet hors de l’instance ${app.forgeName}`),
  );
</script>

{#snippet row(mr: MergeRequest, showAuthor: boolean)}
  {@const actions = actionsOf(mr, !showAuthor)}
  {@const sessions = app.sessionsOf(mr)}
  <li class="mr-row">
    <MrBadge url={mr.webUrl} state={mr.state} />
    <span class="pipeline {mr.pipeline ?? 'none'}" title={mr.pipeline ? PIPELINE[mr.pipeline].label : 'Pas de pipeline lu'}>
      {mr.pipeline ? PIPELINE[mr.pipeline].glyph : '·'}
    </span>
    <div class="main">
      <button
        class="title ellipsis"
        title="Lire dans Robiade (⌘+clic : {app.forgeName})"
        onclick={(e) => app.read({ kind: 'mr', project: mr.project, iid: mr.iid, webUrl: mr.webUrl }, e)}
      >{mr.title}</button>
      <div class="meta">
        <span class="proj" title={mr.project}>{shortProject(mr.project)}</span>
        <span class="mono branches ellipsis" title="{mr.sourceBranch} → {mr.targetBranch}">
          {mr.sourceBranch} → {mr.targetBranch}
        </span>
        {#if mr.wagon}<span class="tag" title="Wagon intermédiaire : pas de squash au merge">wagon</span>{/if}
        {#if showAuthor}<span class="muted">par {mr.author}</span>{/if}
      </div>
    </div>
    <div class="signals">
      {#if mr.hasConflicts}<span class="sig bad">conflits</span>{/if}
      {#if mr.unresolvedDiscussions}<span class="sig warn">discussions</span>{/if}
      {#if mr.approvals && mr.approvals.required > 0}
        {@const a = mr.approvals}
        <span
          class="sig"
          class:good={a.left === 0}
          title={a.by.length ? `Approuvée par ${a.by.join(', ')}` : 'Aucune approbation'}
        >
          {a.left === 0 ? '✓ approuvée' : `${a.required - a.left}/${a.required} approbation${a.required > 1 ? 's' : ''}`}
        </span>
      {/if}
      {#if mr.mergeStatus && !mr.hasConflicts && !(mr.mergeStatus === 'not_approved' && mr.approvals)}
        <span class="sig" class:good={mr.mergeStatus === 'mergeable'}>{MERGE_STATUS[mr.mergeStatus] ?? mr.mergeStatus}</span>
      {/if}
      {#if sessions[0]}
        {@const first = sessions[0]}
        <button class="sig link" title="Ouvrir la session liée" onclick={() => app.openSession(first.sessionId)}>
          {sessions.length} session{sessions.length > 1 ? 's' : ''}
        </button>
      {/if}
    </div>
    <span class="muted num when">{ago(mr.updatedAt, app.now)}</span>
    <div class="actions">
      {#each actions as a (a.label)}
        <button class="btn ghost small" class:merge={a.action.kind === 'merge'} onclick={() => act(mr, a.action)}>{a.label}</button>
      {/each}
    </div>
  </li>
{/snippet}

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Merge requests</h1>
    <span class="muted">{scopeLabel}</span>
    <span class="spacer"></span>
    <button class="btn" onclick={() => void app.refreshForge()} disabled={app.forgeLoading}>
      {app.forgeLoading ? 'Lecture…' : 'Relire'}
    </button>
  </header>

  <div class="scroll">
    {#if app.forgeError}
      <div class="error">
        <strong>{app.forgeName} illisible.</strong>
        <span>{app.forgeError}</span>
        {#if app.forgeError.includes('trousseau')}
          <span class="muted">
            Dans un vrai terminal : <code>security add-generic-password -s {app.env?.forge?.tokenService} -a "$USER" -w</code>
          </span>
        {/if}
      </div>
    {/if}

    {#if !app.env?.forge}
      <p class="muted">Aucune forge réglée : renseigne son instance dans <button class="btn ghost" onclick={() => (app.view = 'settings')}>Réglages</button>.</p>
    {:else if app.forge === null}
      {#if !app.forgeError}<p class="muted">Lecture de {app.forgeName}…</p>{/if}
    {:else}
      <div class="block">
        <h2 class="section-title">À relire · {reviews.length}</h2>
        <ul class="list">
          {#each reviews as mr (mr.webUrl)}
            {@render row(mr, true)}
          {:else}
            <li class="muted">Aucune MR n’attend ta review.</li>
          {/each}
        </ul>
      </div>

      <div class="block">
        <h2 class="section-title">Mes MR ouvertes · {mine.length}</h2>
        <ul class="list">
          {#each mine as mr (mr.webUrl)}
            {@render row(mr, false)}
          {:else}
            <li class="muted">Aucune MR ouverte.</li>
          {/each}
        </ul>
      </div>

      <div class="block">
        <button class="section-title toggle" onclick={() => (showAudit = !showAudit)}>
          {showAudit ? '▾' : '▸'} Journal des actions envoyées
        </button>
        {#if showAudit}
          <ul class="list audit">
            {#each audit as e (`${e.at}-${e.target}-${e.action}`)}
              <li>
                <span class="muted num">{ago(e.at, app.now)}</span>
                <span class="mono">{e.target}</span>
                <span class="grow">{e.action}</span>
                <span class:ok={e.outcome === 'ok'} class:ko={e.outcome !== 'ok'} title={e.outcome}>{e.outcome === 'ok' ? '✓' : `✕ ${e.outcome}`}</span>
              </li>
            {:else}
              <li class="muted">Aucune action envoyée depuis Robiade.</li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </div>
</section>

{#if pending}
  <MrActionConfirm mr={pending.mr} action={pending.action} onclose={() => (pending = null)} />
{/if}

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .scroll { overflow-y: auto; padding: 22px 24px 40px; display: flex; flex-direction: column; gap: 28px; max-width: 1200px; }
  .list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
  .mr-row {
    display: flex; align-items: center; gap: 12px; padding: 8px 10px;
    border-radius: var(--radius-sm); border-bottom: 1px solid var(--line);
  }
  .mr-row:hover { background: var(--bg-hover); }
  .main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .title { border: 0; background: none; padding: 0; text-align: left; font-weight: 550; color: var(--ink); }
  .title:hover { text-decoration: underline; }
  .meta { display: flex; align-items: center; gap: 10px; font-size: 12px; color: var(--ink-2); min-width: 0; }
  .proj { flex: none; }
  .branches { min-width: 0; color: var(--ink-3); font-size: 11.5px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tag {
    flex: none; font-size: 11px; padding: 0 6px; border-radius: 8px;
    border: 1px solid var(--line-strong); color: var(--ink-2);
  }
  .signals { display: flex; gap: 6px; flex: none; align-items: center; }
  .sig {
    font-size: 11.5px; padding: 1px 7px; border-radius: 9px; border: 1px solid var(--line); color: var(--ink-2);
    background: none;
  }
  .sig.good { color: var(--ok); border-color: color-mix(in srgb, var(--ok) 40%, transparent); }
  .sig.warn { color: var(--attention); border-color: color-mix(in srgb, var(--attention) 40%, transparent); }
  .sig.bad { color: var(--danger); border-color: color-mix(in srgb, var(--danger) 40%, transparent); }
  .sig.link { cursor: pointer; }
  .sig.link:hover { background: var(--bg-hover); color: var(--ink); }
  .pipeline {
    width: 20px; height: 20px; flex: none; display: grid; place-items: center; border-radius: 50%;
    font-size: 12px; font-weight: 700; color: var(--ink-3);
  }
  .pipeline.success { color: var(--ok); background: color-mix(in srgb, var(--ok) 14%, transparent); }
  .pipeline.failed { color: var(--danger); background: var(--danger-soft); }
  .pipeline.running { color: var(--mr-open); background: color-mix(in srgb, var(--mr-open) 14%, transparent); }
  .pipeline.manual { color: var(--attention); background: color-mix(in srgb, var(--attention) 14%, transparent); }
  .when { width: 90px; flex: none; text-align: right; }
  .error {
    display: flex; flex-direction: column; gap: 4px; padding: 12px 16px;
    border-left: 3px solid var(--danger); border-radius: var(--radius); background: var(--danger-soft);
  }
  code { font: 12px var(--mono); }
  .actions { display: flex; gap: 4px; flex: none; opacity: 0; }
  .mr-row:hover .actions, .mr-row:focus-within .actions { opacity: 1; }
  .small { height: 24px; padding: 0 8px; font-size: 12px; }
  .merge { color: var(--mr-merged); }
  .toggle { border: 0; background: none; padding: 0; cursor: pointer; }
  .audit li { display: flex; gap: 12px; align-items: baseline; padding: 4px 6px; font-size: 12.5px; }
  .grow { flex: 1; min-width: 0; }
  .ok { color: var(--ok); }
  .ko { color: var(--danger); max-width: 40%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
