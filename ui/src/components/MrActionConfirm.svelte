<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { ActionPreview, MergeRequest, MrAction } from '../lib/types';

  /** `action` sans corps pour un commentaire : il se rédige ici, avant l'aperçu. */
  let { mr, action, onclose }: { mr: MergeRequest; action: MrAction; onclose: () => void } = $props();

  const target = $derived({ project: mr.project, iid: mr.iid });
  let body = $state('');
  let preview = $state<ActionPreview | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  const writing = $derived(action.kind === 'comment' && preview === null);
  const final = $derived<MrAction>(action.kind === 'comment' ? { kind: 'comment', body } : action);
  const risky = $derived(action.kind === 'merge' || action.kind === 'rebase');

  // Aperçu immédiat, sauf pour un commentaire qui doit d'abord être écrit.
  $effect(() => {
    if (action.kind !== 'comment') void loadPreview();
  });

  async function loadPreview() {
    busy = true;
    error = null;
    try {
      preview = await api.previewMrAction(target, final);
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
      const done = await api.runMrAction(target, final);
      app.notify(`${mr.project.split('/').pop()} !${mr.iid} : ${done} — fait.`);
      void app.refreshForge();
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
  <div class="dialog" role="alertdialog" aria-labelledby="act-title">
    {#if writing}
      <h2 id="act-title">Commenter {mr.project.split('/').pop()} !{mr.iid}</h2>
      <!-- svelte-ignore a11y_autofocus -->
      <textarea class="field" rows="6" maxlength="10000" bind:value={body} autofocus placeholder="Commentaire (Markdown GitLab)"></textarea>
      {#if error}<p class="error">{error}</p>{/if}
      <footer>
        <button class="btn" onclick={onclose}>Annuler</button>
        <button class="btn primary" disabled={!body.trim() || busy} onclick={() => void loadPreview()}>Relire avant envoi</button>
      </footer>
    {:else}
      <h2 id="act-title">{preview ? `${preview.action.charAt(0).toUpperCase()}${preview.action.slice(1)} ?` : 'Lecture de la MR sur GitLab…'}</h2>
      {#if preview}
        <dl>
          <dt>Projet</dt><dd class="mono">{preview.project}</dd>
          <dt>MR</dt><dd><strong>!{preview.iid}</strong> {preview.title}</dd>
          <dt>Branches</dt><dd class="mono">{preview.sourceBranch} → {preview.targetBranch}</dd>
          <dt>Auteur</dt><dd>{preview.author}</dd>
          <dt>Action</dt><dd><strong>{preview.action}</strong></dd>
        </dl>
        {#if action.kind === 'comment'}<pre class="body">{body.trim()}</pre>{/if}
      {/if}
      {#if error}<p class="error">{error}</p>{/if}
      <footer>
        <button class="btn" onclick={onclose}>Annuler</button>
        <button class="btn" class:danger={risky} class:primary={!risky} disabled={!preview || busy} onclick={() => void confirm()}>
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
  textarea { resize: vertical; font: inherit; padding: 8px 10px; height: auto; }
  .body {
    margin: 0; max-height: 200px; overflow: auto; padding: 8px 10px; border-radius: var(--radius-sm);
    background: var(--bg); font: 12px var(--mono); white-space: pre-wrap;
  }
  .error { margin: 0; color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .danger { background: var(--danger); border-color: transparent; color: #fff; font-weight: 550; }
</style>
