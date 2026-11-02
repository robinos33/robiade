<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { shortPath } from '../lib/format';
  import StatusBadge from './StatusBadge.svelte';

  const agents = $derived(app.quitRequest?.agents ?? []);
  const relaunch = $derived(app.quitRequest?.relaunch ?? false);
  let cancel: HTMLButtonElement | undefined = $state();

  $effect(() => cancel?.focus());

  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    app.quitRequest = null;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation">
  <div class="dialog" role="alertdialog" aria-labelledby="quit-title" aria-describedby="quit-desc">
    <header>
      <h2 id="quit-title">{relaunch ? 'Relancer' : 'Quitter'} Robiade et arrêter ces agents ?</h2>
      <p id="quit-desc" class="muted">
        Leur travail en cours est interrompu. Les sessions restent reprenables depuis l'historique.
      </p>
    </header>
    <ul>
      {#each agents as a (a.id)}
        <li>
          <StatusBadge status={a.status} />
          <strong>{a.projectName}</strong>
          <span class="mono muted path">{shortPath(a.projectPath, app.env?.home ?? '')}</span>
        </li>
      {/each}
    </ul>
    <footer>
      <button class="btn" bind:this={cancel} onclick={() => (app.quitRequest = null)}>Annuler</button>
      <button class="btn danger" onclick={() => void api.quit(relaunch)}>
        Arrêter {agents.length} agent{agents.length > 1 ? 's' : ''} et {relaunch ? 'relancer' : 'quitter'}
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
    width: min(520px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 14px;
  }
  h2 { margin: 0 0 4px; font-size: 15px; }
  p { margin: 0; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  li { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .path { margin-left: auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 55%; }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .danger { background: var(--danger); border-color: transparent; color: #fff; font-weight: 550; }
  .danger:hover { filter: brightness(1.08); }
</style>
