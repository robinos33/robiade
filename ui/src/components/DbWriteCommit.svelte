<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import StageBadge from './StageBadge.svelte';

  let {
    statement,
    affected,
    delayMs,
    connection,
    target,
    busy,
    onfinish,
  }: {
    statement: string;
    affected: number;
    delayMs: number;
    connection: string;
    target: string;
    busy: boolean;
    onfinish: (commit: boolean) => void;
  } = $props();

  /** Ce que fait l'instruction, pour dire « 3 lignes supprimées » plutôt que « touchées ». */
  const verb = $derived.by(() => {
    const first = statement.replace(/^(\s*(--[^\n]*\n|\/\*[\s\S]*?\*\/))*\s*/, '').split(/\s/)[0]?.toUpperCase();
    return first === 'INSERT' ? 'insérée' : first === 'DELETE' ? 'supprimée' : first === 'UPDATE' ? 'modifiée' : 'touchée';
  });
  const plural = $derived(affected > 1 ? 's' : '');

  // Le serveur annule au bout du délai ; ce décompte ne fait que le montrer. Échéance fixée à
  // l'ouverture, une fois pour toutes.
  const deadline = untrack(() => Date.now() + delayMs);
  let left = $state(untrack(() => Math.ceil(delayMs / 1000)));
  const tick = setInterval(() => (left = Math.max(0, Math.ceil((deadline - Date.now()) / 1000))), 250);
  onDestroy(() => clearInterval(tick));

  let rollback: HTMLButtonElement | undefined = $state();
  // Annuler a le focus : un Entrée réflexe ne valide pas.
  $effect(() => rollback?.focus());

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !busy) {
      e.preventDefault();
      onfinish(false);
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation">
  <div class="dialog" role="alertdialog" aria-labelledby="wc-title">
    <h2 id="wc-title">{affected} ligne{plural} {verb}{plural} — valider ?</h2>
    <div class="grid">
      <span class="k">Connexion</span>
      <span class="path"><StageBadge stage="prod" /> {connection}</span>
      <span class="k">Cible</span>
      <span class="mono path">{target}</span>
    </div>
    <pre class="mono statement">{statement}</pre>
    {#if left > 0}
      <p class="note">Rien n'est encore écrit. Annulée d'office dans <strong>{left} s</strong>.</p>
    {:else}
      <p class="note error">Délai dépassé : la modification a été annulée.</p>
    {/if}
    <footer>
      <button class="btn" bind:this={rollback} disabled={busy} onclick={() => onfinish(false)}>
        {left > 0 ? 'Annuler (ROLLBACK)' : 'Fermer'}
      </button>
      <button class="btn danger" disabled={busy || left === 0} onclick={() => onfinish(true)}>
        Valider (COMMIT)
      </button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 14vh; background: rgb(0 0 0 / 0.45); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(640px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 12px; border-top: 4px solid var(--danger);
  }
  h2 { margin: 0; font-size: 16px; color: var(--danger); }
  .grid { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; align-items: baseline; }
  .k { color: var(--ink-3); font-size: 12.5px; }
  .path { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; }
  .statement {
    margin: 0; max-height: 30vh; overflow: auto; padding: 10px 12px; border-radius: var(--radius-sm);
    background: var(--bg-sunken); border: 1px solid var(--line); white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .note { margin: 0; font-size: 12.5px; }
  .error { color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .danger { background: var(--danger); border-color: transparent; color: #fff; font-weight: 550; }
  .danger:hover:not(:disabled) { filter: brightness(1.08); }
  .danger:disabled { opacity: 0.45; }
</style>
