<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';

  const update = $derived(app.update);
  let showOutput = $state(false);

  const commits = (n: number) => `${n} commit${n > 1 ? 's' : ''}`;

  function onkeydown(e: KeyboardEvent) {
    if (!showOutput || e.key !== 'Escape') return;
    showOutput = false;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

{#if update?.state === 'installing'}
  <p class="update muted" title="Construction d'origin/main dans un worktree à part">
    <span class="spinner" aria-hidden="true"></span>
    <span class="text">Mise à jour en cours<small>{commits(update.commits)}</small></span>
  </p>
{:else if update?.state === 'ready'}
  <button class="update ready" onclick={() => void api.relaunch()}>
    <span class="text">Relancer pour mettre à jour<small>{commits(update.commits)}</small></span>
  </button>
{:else if update?.state === 'failed'}
  <button class="update failed" onclick={() => (showOutput = true)}>
    <span class="text">Mise à jour échouée<small>Voir la sortie</small></span>
  </button>
{/if}

{#if showOutput && update?.state === 'failed'}
  <div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && (showOutput = false)}>
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="update-title">
      <h2 id="update-title">La mise à jour a échoué</h2>
      <p class="muted">L'app installée n'a pas changé. Elle réessaiera au prochain lancement.</p>
      <pre class="mono">{update.output}</pre>
      <footer><button class="btn" onclick={() => (showOutput = false)}>Fermer</button></footer>
    </div>
  </div>
{/if}

<style>
  .update {
    display: flex; align-items: center; gap: 8px; width: 100%; margin: 10px 0 0; padding: 6px 8px;
    border-radius: var(--radius-sm); font-size: 12px; text-align: left;
  }
  p.update { border: 0; }
  .text { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  small { font-size: 11px; font-weight: 400; color: var(--ink-3); }
  .failed small { color: inherit; opacity: 0.8; }
  button.update { border: 1px solid var(--line-strong); background: var(--bg); color: var(--ink); }
  .ready { border-color: var(--accent) !important; font-weight: 550; }
  .ready:hover { background: var(--bg-hover); }
  .failed { color: var(--danger) !important; border-color: var(--danger-soft) !important; background: var(--danger-soft) !important; }
  .spinner {
    width: 10px; height: 10px; flex: none; border-radius: 50%;
    border: 1.5px solid var(--line-strong); border-top-color: var(--ink-3); animation: spin 0.9s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 12vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(760px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 10px;
  }
  h2 { margin: 0; font-size: 15px; }
  p { margin: 0; }
  pre {
    margin: 0; max-height: 50vh; overflow: auto; padding: 10px; border-radius: var(--radius-sm);
    background: var(--bg); border: 1px solid var(--line); font-size: 11.5px; white-space: pre-wrap; word-break: break-word;
  }
  footer { display: flex; justify-content: flex-end; }
</style>
