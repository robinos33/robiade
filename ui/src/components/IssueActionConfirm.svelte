<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { Issue, IssueAction, IssueActionPreview } from '../lib/types';

  /** `ondone` reçoit l'issue relue après l'envoi, pour mettre la liste à jour sans attendre GitLab. */
  let { issue, action, onclose, ondone }: {
    issue: Issue;
    action: IssueAction;
    onclose: () => void;
    ondone: (issue: Issue) => void;
  } = $props();

  const target = $derived({ project: issue.project, iid: issue.iid });
  let preview = $state<IssueActionPreview | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);

  $effect(() => {
    void loadPreview();
  });

  async function loadPreview() {
    busy = true;
    error = null;
    try {
      preview = await api.previewIssueAction(target, action);
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
      const done = await api.runIssueAction(target, action);
      app.notify(`${issue.project.split('/').pop()} #${issue.iid} : ${done} — fait.`);
      ondone({ ...issue, state: action.kind === 'close' ? 'closed' : 'opened' });
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
    <h2 id="issue-act-title">
      {preview ? `${preview.action.charAt(0).toUpperCase()}${preview.action.slice(1)} ?` : "Lecture de l'issue sur GitLab…"}
    </h2>
    {#if preview}
      <dl>
        <dt>Projet</dt><dd class="mono">{preview.project}</dd>
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
  .error { margin: 0; color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
</style>
