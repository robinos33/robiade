<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { FileChange, Worktree, WorktreeRemoval } from '../lib/types';

  let { worktree, onclose }: { worktree: Worktree; onclose: () => void } = $props();

  let removal = $state<WorktreeRemoval | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  /** Perte des modifications non commitées acceptée, case cochée. */
  let discard = $state(false);
  let cancel: HTMLButtonElement | undefined = $state();

  $effect(() => cancel?.focus());

  $effect(() => {
    api.prepareWorktreeRemoval(worktree.path).then(
      (r) => (removal = r),
      (e) => (error = String(e)),
    );
  });

  const LETTER: Record<FileChange['kind'], string> = {
    modified: 'M', added: 'A', deleted: 'D', renamed: 'R', untracked: 'U',
  };

  /** Montre dans le Finder un fichier du worktree, ou le worktree lui-même si `path` est vide. */
  function reveal(path: string) {
    api.revealInFinder(worktree.path, path).catch((e) => (error = String(e)));
  }

  async function remove() {
    busy = true;
    error = null;
    try {
      await api.removeWorktree(worktree.path, discard ? (removal?.changeCount ?? 0) : 0);
      if (app.worktree === worktree.path) app.selectProject(worktree.root);
      await Promise.all([app.refreshHistory(), app.refreshWorktrees()]);
      app.notify(`Worktree ${worktree.name} supprimé${removal?.branch ? `, branche ${removal.branch} conservée` : ''}.`);
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
  <div class="dialog" role="alertdialog" aria-labelledby="wt-title">
    <h2 id="wt-title">Supprimer le worktree {worktree.name} ?</h2>
    {#if !removal}
      <p class="muted">{error ? '' : 'Lecture du worktree…'}</p>
    {:else}
      <div class="grid">
        <span class="k">Dossier</span>
        <button class="link mono path" onclick={() => reveal('')} title="Ouvrir dans le Finder">{removal.path}</button>
        <span class="k">Dépôt</span>
        <span class="mono path">{removal.repository}</span>
        <span class="k">Branche</span>
        <span class="mono path">{removal.branch ?? 'tête détachée'}</span>
      </div>
      {#if removal.blocker}
        <p class="error">{removal.blocker}</p>
      {:else if removal.changeCount > 0}
        <p class="error">
          {removal.changeCount} fichier(s) modifié(s) ou non suivi(s), dans aucun commit : la suppression les efface.
        </p>
        <ul class="changes">
          {#each removal.changes as c (c.path)}
            <li>
              <button class="link" onclick={() => reveal(c.path)} title="Montrer dans le Finder">
                <span class="letter {c.kind}">{LETTER[c.kind]}</span>
                <span class="mono">{c.path}</span>
              </button>
            </li>
          {/each}
        </ul>
        {#if removal.changeCount > removal.changes.length}
          <p class="muted note">… et {removal.changeCount - removal.changes.length} autre(s).</p>
        {/if}
        <label class="discard">
          <input type="checkbox" bind:checked={discard} />
          Perdre ces {removal.changeCount} modification(s){removal.branch ? `, la branche ${removal.branch} est conservée` : ''}
        </label>
      {:else}
        <p class="muted note">
          Le dossier est effacé. {removal.branch ? `La branche ${removal.branch} est conservée` : 'Aucune branche n’est touchée'} ;
          les sessions restent dans l'historique.
        </p>
      {/if}
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    <footer>
      <button class="btn" bind:this={cancel} onclick={onclose}>Annuler</button>
      <button
        class="btn danger"
        disabled={!removal || !!removal.blocker || (removal.changeCount > 0 && !discard) || busy}
        onclick={() => void remove()}
      >
        {busy ? 'Suppression…' : removal?.changeCount ? 'Supprimer avec les modifications' : 'Supprimer le worktree'}
      </button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 18vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(560px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 12px;
  }
  h2 { margin: 0; font-size: 15px; overflow-wrap: anywhere; }
  .grid { display: grid; grid-template-columns: 70px 1fr; gap: 6px 12px; align-items: baseline; }
  .k { color: var(--ink-3); font-size: 12.5px; }
  .path { font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; }
  .note { margin: 0; font-size: 12px; }
  .link {
    all: unset; cursor: pointer; text-align: left; overflow-wrap: anywhere;
    text-decoration: underline dotted var(--ink-3); text-underline-offset: 3px;
  }
  .link:hover { color: var(--ink); text-decoration-style: solid; }
  .link:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 3px; }
  .changes {
    margin: -4px 0 0; padding: 0; list-style: none; max-height: 180px; overflow: auto;
    display: flex; flex-direction: column; gap: 3px; font-size: 12px;
  }
  .changes .link { display: flex; gap: 8px; align-items: baseline; text-decoration: none; color: var(--ink-2); }
  .changes .link:hover .mono { text-decoration: underline; }
  .letter { font: 600 10.5px var(--mono); width: 12px; flex: none; color: var(--ink-3); }
  .letter.added, .letter.untracked { color: var(--ok); }
  .letter.deleted { color: var(--danger); }
  .letter.renamed { color: var(--info); }
  .error { margin: 0; color: var(--danger); }
  .discard { display: flex; gap: 8px; align-items: center; font-size: 12.5px; cursor: pointer; }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .danger { background: var(--danger); border-color: transparent; color: #fff; font-weight: 550; }
  .danger:hover:not(:disabled) { filter: brightness(1.08); }
  .danger:disabled { opacity: 0.5; }
</style>
