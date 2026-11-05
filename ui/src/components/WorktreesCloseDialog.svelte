<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { Worktree } from '../lib/types';
  import WorktreeBranch from './WorktreeBranch.svelte';
  import ProjectTag from './ProjectTag.svelte';

  /** Les worktrees propres au moment du clic ; le serveur revérifie chacun avant de le supprimer. */
  let { worktrees, onclose }: { worktrees: Worktree[]; onclose: () => void } = $props();

  let busy = $state(false);
  let done = $state(0);
  /** Bilan d'une fermeture où au moins un worktree a été refusé ; `null` tant qu'elle n'a pas eu lieu. */
  let report = $state<{ closed: number; refused: { worktree: Worktree; reason: string }[] } | null>(null);
  let cancel: HTMLButtonElement | undefined = $state();

  $effect(() => cancel?.focus());

  const closedLabel = (n: number) => `${n} worktree${n > 1 ? 's' : ''} fermé${n > 1 ? 's' : ''}`;

  /** Un par un : chaque appel refait les vérifications du dialogue unitaire, avec 0 modification à perdre. */
  async function closeAll() {
    busy = true;
    const refused: { worktree: Worktree; reason: string }[] = [];
    const closed: Worktree[] = [];
    for (const w of worktrees) {
      try {
        await api.removeWorktree(w.path, 0);
        closed.push(w);
      } catch (e) {
        refused.push({ worktree: w, reason: String(e) });
      }
      done++;
    }
    const selected = closed.find((w) => w.path === app.worktree);
    if (selected) app.selectProject(selected.root);
    await Promise.all([app.refreshHistory(), app.refreshWorktrees()]);
    busy = false;
    if (refused.length === 0) {
      app.notify(`${closedLabel(closed.length)}, branches conservées.`);
      onclose();
    } else {
      // Un toast disparaît en quelques secondes : les refus et leur raison restent affichés ici.
      report = { closed: closed.length, refused };
    }
  }

  function onkeydown(e: KeyboardEvent) {
    // Pendant la boucle, fermer le dialogue n'arrêterait rien : on attend la fin.
    if (e.key === 'Escape' && !busy) {
      e.preventDefault();
      onclose();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation">
  <div class="dialog" role="alertdialog" aria-labelledby="wts-title">
    {#if report}
      <h2 id="wts-title">{closedLabel(report.closed)}, {report.refused.length} refusé{report.refused.length > 1 ? 's' : ''}</h2>
      <ul class="list">
        {#each report.refused as r (r.worktree.path)}
          <li class="refused">
            <span class="name ellipsis" title={r.worktree.path}>{r.worktree.name}</span>
            <span class="error">{r.reason}</span>
          </li>
        {/each}
      </ul>
      <footer><button class="btn" bind:this={cancel} onclick={onclose}>Terminé</button></footer>
    {:else}
      <h2 id="wts-title">Fermer {worktrees.length} worktree{worktrees.length > 1 ? 's' : ''} propre{worktrees.length > 1 ? 's' : ''} ?</h2>
      <ul class="list">
        {#each worktrees as w (w.path)}
          <li class="item">
            <span class="name ellipsis" title={w.path}>{w.name}</span>
            <WorktreeBranch git={w.git} />
            {#if app.project === null}<span class="muted ellipsis"><ProjectTag name={w.projectName} root={w.root} /></span>{/if}
          </li>
        {/each}
      </ul>
      <p class="muted note">
        Aucun n'a de modification non commitée : les dossiers sont effacés, les branches conservées ;
        les sessions restent dans l'historique.
      </p>
      <footer>
        <button class="btn" bind:this={cancel} onclick={onclose} disabled={busy}>Annuler</button>
        <button class="btn danger" disabled={busy} onclick={() => void closeAll()}>
          {busy ? `Fermeture ${done}/${worktrees.length}…` : `Fermer ${worktrees.length > 1 ? `les ${worktrees.length} worktrees` : 'le worktree'}`}
        </button>
      </footer>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 18vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(620px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 12px;
  }
  h2 { margin: 0; font-size: 15px; overflow-wrap: anywhere; }
  .list {
    margin: 0; padding: 0; list-style: none; max-height: 260px; overflow: auto;
    display: flex; flex-direction: column; gap: 4px;
  }
  .item { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto; gap: 12px; align-items: center; }
  .refused { display: flex; flex-direction: column; gap: 2px; }
  .name { font-weight: 550; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .note { margin: 0; font-size: 12px; }
  .error { margin: 0; color: var(--danger); font-size: 12px; overflow-wrap: anywhere; }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .danger { background: var(--danger); border-color: transparent; color: #fff; font-weight: 550; }
  .danger:hover:not(:disabled) { filter: brightness(1.08); }
  .danger:disabled { opacity: 0.5; }
</style>
