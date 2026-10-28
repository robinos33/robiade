<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { NewIssue } from '../lib/types';

  /** `oncreated` reçoit le numéro attribué par GitLab, pour relire la liste. */
  let { project, onclose, oncreated }: { project: string; onclose: () => void; oncreated: (iid: number) => void } =
    $props();

  let title = $state('');
  let description = $state('');
  let labels = $state('');
  let assignees = $state('');
  /** Description produite par le domaine ; `null` tant que la saisie n'a pas été relue. */
  let preview = $state<string | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);

  const list = (text: string) => text.split(',').map((s) => s.trim()).filter(Boolean);
  const issue = $derived<NewIssue>({ project, title, description, labels: list(labels), assignees: list(assignees) });

  async function loadPreview() {
    busy = true;
    error = null;
    try {
      preview = await api.previewIssueCreation(issue);
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
      const created = await api.createIssue(issue);
      app.notify(`${project.split('/').pop()} #${created.iid} : issue créée.`);
      oncreated(created.iid);
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
  <div class="dialog" role="alertdialog" aria-labelledby="new-issue-title">
    {#if preview === null}
      <h2 id="new-issue-title">Nouvelle issue dans <span class="mono">{project}</span></h2>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="field" maxlength="255" bind:value={title} autofocus placeholder="Titre" />
      <textarea class="field" rows="8" maxlength="50000" bind:value={description} placeholder="Description (Markdown GitLab)"></textarea>
      <input class="field" bind:value={labels} placeholder="Labels, séparés par des virgules" />
      <input class="field" bind:value={assignees} placeholder="Assignés, noms d’utilisateur séparés par des virgules" />
      {#if error}<p class="error">{error}</p>{/if}
      <footer>
        <button class="btn" onclick={onclose}>Annuler</button>
        <button class="btn primary" disabled={!title.trim() || busy} onclick={() => void loadPreview()}>Relire avant envoi</button>
      </footer>
    {:else}
      <h2 id="new-issue-title">{preview.charAt(0).toUpperCase()}{preview.slice(1)} ?</h2>
      <dl>
        <dt>Projet</dt><dd class="mono">{project}</dd>
        <dt>Titre</dt><dd><strong>{title.trim()}</strong></dd>
      </dl>
      {#if description.trim()}<pre class="body">{description.trim()}</pre>{/if}
      {#if error}<p class="error">{error}</p>{/if}
      <footer>
        <button class="btn" onclick={() => (preview = null)}>Modifier</button>
        <button class="btn primary" disabled={busy} onclick={() => void confirm()}>{busy ? 'Envoi…' : 'Créer'}</button>
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
    padding: 18px; display: flex; flex-direction: column; gap: 10px;
  }
  h2 { margin: 0 0 4px; font-size: 15px; }
  dl { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; margin: 0; }
  dt { color: var(--ink-3); }
  dd { margin: 0; min-width: 0; overflow-wrap: anywhere; }
  textarea { resize: vertical; font: inherit; padding: 8px 10px; height: auto; }
  .body {
    margin: 0; max-height: 240px; overflow: auto; padding: 8px 10px; border-radius: var(--radius-sm);
    background: var(--bg); font: 12px var(--mono); white-space: pre-wrap;
  }
  .error { margin: 0; color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
</style>
