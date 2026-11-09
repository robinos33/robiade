<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { pasteText } from '../lib/terminals';
  import type { Agent, AgentStatus, Shortcut } from '../lib/types';

  let { agent }: { agent: Agent } = $props();

  let open = $state(false);
  let more: HTMLElement | undefined = $state();

  /**
   * Collage permis seulement quand l'agent attend un prompt. Pendant une demande de permission, le
   * menu de Claude Code lit les touches : un collage pourrait y répondre. Pendant un tour, un menu
   * (`AskUserQuestion`) peut être ouvert sans qu'aucun hook ne le dise.
   */
  const ready = $derived(agent.status === 'awaitingInput' || agent.status === 'idle');
  const WHY: Record<AgentStatus, string> = {
    starting: 'L’agent démarre',
    running: 'L’agent travaille : un menu peut être ouvert dans le terminal',
    awaitingPermission: 'Une demande de permission lit les touches : un collage pourrait y répondre',
    awaitingInput: '',
    idle: '',
    failed: 'Le dernier tour a échoué : relance l’agent ou tape dans le terminal',
    ended: 'Agent terminé',
  };

  function use(s: Shortcut) {
    open = false;
    if (ready) pasteText(agent.id, s.pasted);
  }

  // Le menu se ferme au clic ailleurs et à Échap, qui n'atteint alors pas le terminal.
  $effect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!more?.contains(e.target as Node)) open = false;
    };
    const escape = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.stopPropagation();
      open = false;
    };
    window.addEventListener('pointerdown', away, true);
    window.addEventListener('keydown', escape, true);
    return () => {
      window.removeEventListener('pointerdown', away, true);
      window.removeEventListener('keydown', escape, true);
    };
  });
  $effect(() => {
    if (!ready) open = false;
  });
</script>

{#if app.shortcuts.length}
  <div class="shortcuts" role="toolbar" aria-label="Raccourcis">
    <div class="inline">
      {#each app.shortcuts as s, i (i)}
        <button class="btn ghost" aria-disabled={!ready} title={ready ? s.text : WHY[agent.status]} onclick={() => use(s)}>
          {s.label}
        </button>
      {/each}
    </div>
    <div class="more" bind:this={more}>
      <button
        class="btn ghost"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-disabled={!ready}
        title={ready ? 'Tous les raccourcis' : WHY[agent.status]}
        onclick={() => (open = ready && !open)}
      >…</button>
      {#if open}
        <div class="menu" role="menu">
          {#each app.shortcuts as s, i (i)}
            <button role="menuitem" title={s.text} onclick={() => use(s)}>{s.label}</button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  /* La frise passe d'abord : les raccourcis prennent la place qui reste. Ceux qui n'y tiennent pas
     passent à la ligne, hors de la bande, et « … » les donne tous. */
  .shortcuts { flex: 1 1 0; display: flex; align-items: center; justify-content: flex-end; min-width: 0; }
  .inline {
    flex: 0 1 auto; display: flex; flex-wrap: wrap; justify-content: flex-end; align-items: center;
    gap: 2px; min-width: 0; height: 22px; overflow: hidden;
  }
  .btn { height: 22px; padding: 0 8px; font-size: 12px; font-family: var(--mono); white-space: nowrap; }
  .btn[aria-disabled='true'] { opacity: 0.45; cursor: default; }
  .btn[aria-disabled='true']:hover { background: transparent; }
  .more { flex: none; position: relative; }
  .menu {
    position: absolute; right: 0; bottom: calc(100% + 6px); z-index: 90; min-width: 180px; max-height: 60vh;
    overflow-y: auto; padding: 4px; border-radius: 8px;
    background: var(--bg-raised); border: 1px solid var(--line-strong); box-shadow: var(--shadow);
  }
  .menu button {
    display: block; width: 100%; padding: 6px 10px; border: 0; border-radius: var(--radius-sm);
    background: none; text-align: left; color: var(--ink); font: 12px var(--mono); white-space: nowrap;
  }
  .menu button:hover { background: var(--bg-hover); }

</style>
