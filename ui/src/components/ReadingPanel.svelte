<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { untrack } from 'svelte';
  import { cubicOut } from 'svelte/easing';
  import { prefersReducedMotion } from 'svelte/motion';
  import { slide } from 'svelte/transition';
  import { api } from '../lib/api';
  import { ago } from '../lib/format';
  import { MERGE_STATUS, PIPELINE, actionsOf } from '../lib/mergeRequests';
  import { app } from '../lib/state.svelte';
  import { threadPage } from '../lib/thread';
  import type { IssueReading, MergeRequest, MrAction, MrReading, ReadingTarget } from '../lib/types';
  import MrActionConfirm from './MrActionConfirm.svelte';

  /**
   * Panneau de lecture d'une issue ou d'une MR, à côté de la liste : en-tête lu comme du texte,
   * description et échanges rendus dans une iframe `sandbox=""`. Relu à chaque ouverture, rien
   * n'est gardé.
   */
  let { target }: { target: ReadingTarget } = $props();

  let reading = $state<{ key: string; mr: MrReading | null; issue: IssueReading | null } | null>(null);
  let error = $state<string | null>(null);
  /** Incrémenté pour relire, après une action sur la MR. */
  let reload = $state(0);
  let pending = $state<MrAction | null>(null);
  /** Dernière lecture lancée : une réponse plus ancienne est ignorée. */
  let latest = 0;

  const key = $derived(`${target.kind}:${target.project}!${target.iid}`);

  $effect(() => {
    const t = target;
    const k = key;
    void reload;
    const seq = ++latest;
    // Une relecture garde l'affichage ; un autre élément le vide.
    if (untrack(() => reading?.key) !== k) reading = null;
    error = null;
    const ref = { project: t.project, iid: t.iid };
    const read =
      t.kind === 'mr'
        ? api.readMergeRequest(ref).then((mr) => ({ key: k, mr, issue: null }))
        : api.readIssue(ref).then((issue) => ({ key: k, mr: null, issue }));
    read.then(
      (r) => seq === latest && (reading = r),
      (e) => seq === latest && (error = String(e)),
    );
  });

  const page = $derived.by(() => {
    if (!reading) return null;
    const r = reading.mr ?? reading.issue;
    return r && threadPage(target.project, r.description, r.discussions);
  });

  const mr = $derived(reading?.mr?.mr ?? null);
  const issue = $derived(reading?.issue?.issue ?? null);
  const actions = $derived(
    mr && reading?.mr && (mr.state === 'opened' || mr.state === 'draft') ? actionsOf(mr, reading.mr.mine) : [],
  );
  const MR_STATE = { draft: 'Draft', opened: 'Ouverte', merged: 'Mergée', closed: 'Fermée' };

  /** Statut de merge déjà dit par un autre signal de l'en-tête. */
  const said = (m: MergeRequest) =>
    (m.mergeStatus === 'discussions_not_resolved' && m.unresolvedDiscussions) ||
    (m.mergeStatus === 'not_approved' && m.approvals !== null);

  const duration = $derived(prefersReducedMotion.current ? 0 : 200);

  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape' || pending) return;
    // Échap appartient au champ, au terminal ou à la modale qui a la main.
    if (e.target instanceof Element && e.target.closest('input, textarea, select, [contenteditable="true"]')) return;
    if (document.querySelector('[role="dialog"], [role="alertdialog"]')) return;
    app.reading = null;
  }
</script>

<svelte:window {onkeydown} />

<aside class="panel" aria-label="Lecture" transition:slide={{ axis: 'x', duration, easing: cubicOut }}>
  <div class="inner">
    <header>
      <div class="top">
        <span class="ref mono">{target.kind === 'mr' ? '!' : '#'}{target.iid}</span>
        <span class="proj ellipsis" title={target.project}>{target.project}</span>
        <span class="spacer"></span>
        <button class="btn ghost small" title="Ouvrir dans le navigateur" onclick={() => void openUrl(target.webUrl)}>
          Ouvrir dans GitLab
        </button>
        <button class="btn ghost small close" title="Fermer (Échap)" aria-label="Fermer" onclick={() => (app.reading = null)}>✕</button>
      </div>

      {#if mr}
        <h2 class="selectable">{mr.title}</h2>
        <div class="meta">
          <span class="state {mr.state}">{MR_STATE[mr.state]}</span>
          <span class="mono branches ellipsis" title="{mr.sourceBranch} → {mr.targetBranch}">{mr.sourceBranch} → {mr.targetBranch}</span>
          {#if mr.wagon}<span class="tag" title="Wagon intermédiaire : pas de squash au merge">wagon</span>{/if}
          <span class="muted">par {mr.author} · {ago(mr.updatedAt, app.now)}</span>
        </div>
        <div class="meta">
          {#if mr.pipeline}<span class="sig pipeline {mr.pipeline}">{PIPELINE[mr.pipeline].glyph} {PIPELINE[mr.pipeline].label}</span>{/if}
          {#if mr.hasConflicts}<span class="sig bad">conflits</span>{/if}
          {#if mr.unresolvedDiscussions}<span class="sig warn">discussions ouvertes</span>{/if}
          {#if mr.approvals && mr.approvals.required > 0}
            {@const a = mr.approvals}
            <span class="sig" class:good={a.left === 0} title={a.by.length ? `Approuvée par ${a.by.join(', ')}` : 'Aucune approbation'}>
              {a.left === 0 ? '✓ approuvée' : `${a.required - a.left}/${a.required} approbation${a.required > 1 ? 's' : ''}`}
            </span>
          {/if}
          {#if mr.mergeStatus && mr.state !== 'merged' && mr.state !== 'closed' && !mr.hasConflicts && !said(mr)}
            <span class="sig" class:good={mr.mergeStatus === 'mergeable'}>{MERGE_STATUS[mr.mergeStatus] ?? mr.mergeStatus}</span>
          {/if}
        </div>
        {#if actions.length}
          <div class="actions">
            {#each actions as a (a.label)}
              <button class="btn ghost small" class:merge={a.action.kind === 'merge'} onclick={() => (pending = a.action)}>{a.label}</button>
            {/each}
          </div>
        {/if}
      {:else if issue}
        <h2 class="selectable" class:closed={issue.state === 'closed'}>{issue.title}</h2>
        <div class="meta">
          <span class="state {issue.state}">{issue.state === 'opened' ? 'Ouverte' : 'Fermée'}</span>
          {#each issue.labels as l (l)}<span class="tag">{l}</span>{/each}
          {#if issue.assignees.length}<span class="muted">assignée à {issue.assignees.join(', ')}</span>{/if}
          {#if issue.dueDate}<span class="due num" class:late={issue.state === 'opened' && issue.dueDate < app.today}>échéance {issue.dueDate}</span>{/if}
          <span class="muted">{ago(issue.updatedAt, app.now)}</span>
        </div>
      {/if}
    </header>

    {#if error}
      <p class="error">GitLab illisible : {error}</p>
    {:else if page === null}
      <p class="muted pad">Lecture de GitLab…</p>
    {:else}
      <!-- Sans `allow-scripts` ni `allow-same-origin` : le contenu GitLab s'affiche, rien ne s'y exécute. -->
      <iframe title="Description et échanges" sandbox="" srcdoc={page}></iframe>
    {/if}
  </div>
</aside>

{#if pending && mr}
  <MrActionConfirm
    {mr}
    action={pending}
    onclose={() => {
      pending = null;
      reload++;
    }}
  />
{/if}

<style>
  .panel {
    --w: min(560px, 46vw);
    flex: none; width: var(--w); overflow: hidden;
    border-left: 1px solid var(--line); background: var(--bg-raised);
  }
  /* Largeur fixe : pendant l'animation, le panneau se découvre au lieu de se recomposer. */
  .inner { width: var(--w); height: 100%; display: flex; flex-direction: column; }
  header { flex: none; display: flex; flex-direction: column; gap: 8px; padding: 12px 18px 14px; border-bottom: 1px solid var(--line); }
  .top { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .ref { color: var(--ink-3); font-size: 12px; }
  .proj { font-size: 12px; color: var(--ink-2); min-width: 0; }
  .spacer { flex: 1; }
  .small { height: 24px; padding: 0 8px; font-size: 12px; flex: none; }
  .close { width: 24px; padding: 0; }
  h2 { margin: 0; font-size: 16px; line-height: 1.35; font-weight: 600; word-break: break-word; }
  h2.closed { color: var(--ink-3); text-decoration: line-through; }
  .meta { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 10px; font-size: 12px; color: var(--ink-2); min-width: 0; }
  .branches { min-width: 0; font-size: 11.5px; color: var(--ink-3); }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .state { --c: var(--ink-2); padding: 0 7px; border-radius: 9px; color: var(--c); background: color-mix(in srgb, var(--c) 14%, transparent); }
  .state.opened { --c: var(--mr-open); }
  .state.draft { --c: var(--mr-draft); }
  .state.merged { --c: var(--mr-merged); }
  .state.closed { --c: var(--mr-closed); }
  .tag { font-size: 11px; padding: 0 6px; border-radius: 8px; border: 1px solid var(--line-strong); color: var(--ink-2); }
  .sig { font-size: 11.5px; padding: 1px 7px; border-radius: 9px; border: 1px solid var(--line); color: var(--ink-2); }
  .sig.good, .pipeline.success { color: var(--ok); border-color: color-mix(in srgb, var(--ok) 40%, transparent); }
  .sig.warn, .pipeline.manual { color: var(--attention); border-color: color-mix(in srgb, var(--attention) 40%, transparent); }
  .sig.bad, .pipeline.failed { color: var(--danger); border-color: color-mix(in srgb, var(--danger) 40%, transparent); }
  .pipeline.running { color: var(--mr-open); border-color: color-mix(in srgb, var(--mr-open) 40%, transparent); }
  .due.late { color: var(--danger); }
  .actions { display: flex; flex-wrap: wrap; gap: 4px; }
  .merge { color: var(--mr-merged); }
  iframe { flex: 1; min-height: 0; border: 0; background: var(--bg-raised); }
  .pad { padding: 14px 18px; margin: 0; }
  .error { margin: 14px 18px; padding: 10px 14px; border-left: 3px solid var(--danger); border-radius: var(--radius); background: var(--danger-soft); }
</style>
