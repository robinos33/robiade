<script lang="ts">
  import { untrack } from 'svelte';
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { Issue, IssueAction, IssueActionPreview } from '../lib/types';
  import ProjectTag from './ProjectTag.svelte';

  /**
   * Assignés et labels se modifient ici, avant l'aperçu, à partir des valeurs de l'issue.
   * `ondone` reçoit l'issue telle qu'elle devrait être après l'envoi, pour mettre la liste à jour
   * sans attendre la prochaine lecture de GitLab.
   */
  let { issue, action, onclose, ondone }: {
    issue: Issue;
    action: IssueAction;
    onclose: () => void;
    ondone: (issue: Issue) => void;
  } = $props();

  const target = $derived({ project: issue.project, iid: issue.iid });
  const editable = $derived(action.kind === 'assign' || action.kind === 'label');
  /** Liste séparée par des virgules (GitLab refuse la virgule dans un label), partant de l'action. */
  let text = $state(
    untrack(() => (action.kind === 'assign' ? action.assignees : action.kind === 'label' ? action.labels : []).join(', ')),
  );
  const items = $derived(text.split(',').map((s) => s.trim()).filter(Boolean));
  let preview = $state<IssueActionPreview | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  const writing = $derived(editable && preview === null);
  const final = $derived<IssueAction>(
    action.kind === 'assign'
      ? { kind: 'assign', assignees: items }
      : action.kind === 'label'
        ? { kind: 'label', labels: items }
        : action,
  );

  // Aperçu immédiat, sauf quand il faut d'abord saisir les assignés ou les labels.
  $effect(() => {
    if (!editable) void loadPreview();
  });

  function updated(): Issue {
    switch (final.kind) {
      case 'close': return { ...issue, state: 'closed' };
      case 'reopen': return { ...issue, state: 'opened' };
      case 'assign': return { ...issue, assignees: final.assignees.map((a) => a.replace(/^@/, '')) };
      case 'label': return { ...issue, labels: final.labels };
    }
  }

  async function loadPreview() {
    busy = true;
    error = null;
    try {
      preview = await api.previewIssueAction(target, final);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function confirm() {
    busy = true;
    error = null;
    try {
      const done = await api.runIssueAction(target, final);
      app.notify(`${issue.project.split('/').pop()} #${issue.iid} : ${done} — fait.`);
      ondone(updated());
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation">
  <div class="dialog" role="alertdialog" aria-labelledby="issue-act-title">
    {#if writing}
      <h2 id="issue-act-title">
        {action.kind === 'assign' ? 'Assignés de' : 'Labels de'} {issue.project.split('/').pop()} #{issue.iid}
      </h2>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="field"
        bind:value={text}
        autofocus
        placeholder={action.kind === 'assign' ? 'Noms d’utilisateur, séparés par des virgules' : 'Labels, séparés par des virgules'}
        onkeydown={(e) => e.key === 'Enter' && !busy && void loadPreview()}
      />
      <p class="hint">Liste complète : ce qui n'y figure plus sera retiré.</p>
      {#if error}<p class="error">{error}</p>{/if}
      <footer>
        <button class="btn" onclick={onclose}>Annuler</button>
        <button class="btn primary" disabled={busy} onclick={() => void loadPreview()}>Relire avant envoi</button>
      </footer>
    {:else}
      <h2 id="issue-act-title">
        {preview ? `${preview.action.charAt(0).toUpperCase()}${preview.action.slice(1)} ?` : `Lecture de l'issue sur ${app.forgeName}…`}
      </h2>
      {#if preview}
        <dl>
          <dt>Projet</dt><dd class="mono"><ProjectTag name={preview.project} forge={preview.project} /></dd>
          <dt>Issue</dt><dd><strong>#{preview.iid}</strong> {preview.title}</dd>
          <dt>État</dt><dd>{preview.state === 'opened' ? 'ouverte' : 'fermée'}</dd>
          <dt>Assignés</dt><dd>{preview.assignees.join(', ') || '—'}</dd>
          <dt>Labels</dt><dd>{preview.labels.join(', ') || '—'}</dd>
          <dt>Action</dt><dd><strong>{preview.action}</strong></dd>
        </dl>
      {/if}
      {#if error}<p class="error">{error}</p>{/if}
      <footer>
        <button class="btn" onclick={onclose}>Annuler</button>
        <button class="btn primary" disabled={!preview || busy} onclick={() => void confirm()}>
          {busy && preview ? 'Envoi…' : 'Confirmer'}
        </button>
      </footer>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 14vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(600px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 14px;
  }
  h2 { margin: 0; font-size: 15px; }
  dl { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; margin: 0; }
  dt { color: var(--ink-3); }
  dd { margin: 0; min-width: 0; overflow-wrap: anywhere; }
  .hint { margin: 0; font-size: 12px; color: var(--ink-3); }
  .error { margin: 0; color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
</style>
