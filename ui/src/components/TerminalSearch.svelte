<script lang="ts">
  import { endFind, find, onFindResults } from '../lib/terminals';
  import Icon from './Icon.svelte';

  /**
   * Recherche dans l'historique du terminal d'un agent (⌘F) : Entrée et ⇧Entrée passent d'une
   * occurrence à l'autre, Échap ferme et rend le clavier au terminal.
   */
  let { id, onclose }: { id: string; onclose: () => void } = $props();

  let query = $state('');
  let results = $state<{ index: number; count: number } | null>(null);
  let input: HTMLInputElement | undefined = $state();

  /** ⌘F, barre déjà ouverte : le champ reprend le clavier, son texte sélectionné. */
  export function focus() {
    input?.select();
  }

  $effect(() => onFindResults(id, (index, count) => (results = { index, count })));
  // Fermée, ou remplacée par celle d'un autre agent : plus de surbrillances.
  $effect(() => () => endFind(id, false));
  $effect(() => input?.focus());

  const counter = $derived.by(() => {
    if (!query || !results) return '';
    if (results.count === 0) return 'Aucun';
    return `${results.index < 0 ? '–' : results.index + 1} / ${results.count}`;
  });

  function close() {
    endFind(id, true);
    onclose();
  }

  function oninput() {
    if (!query) results = null;
    find(id, query, 'typing');
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') find(id, query, e.shiftKey ? 'previous' : 'next');
    else if (e.key === 'Escape') close();
    else return;
    e.preventDefault();
  }
</script>

<div class="search" role="search">
  <input
    class="field"
    bind:this={input}
    bind:value={query}
    {oninput}
    {onkeydown}
    placeholder="Rechercher"
    aria-label="Rechercher dans le terminal"
    spellcheck="false"
  />
  <span class="count muted num">{counter}</span>
  <button class="btn ghost icon" disabled={!query} onclick={() => find(id, query, 'previous')} title="Précédent (⇧Entrée)"><Icon name="prev" size={13} /></button>
  <button class="btn ghost icon" disabled={!query} onclick={() => find(id, query, 'next')} title="Suivant (Entrée)"><Icon name="next" size={13} /></button>
  <button class="btn ghost icon" onclick={close} title="Fermer (Échap)"><Icon name="close" size={13} /></button>
</div>

<style>
  .search {
    position: absolute; top: 8px; right: 16px; z-index: 5; display: flex; align-items: center; gap: 2px;
    padding: 4px; border-radius: 8px; background: var(--bg-raised); box-shadow: var(--shadow);
  }
  .field { width: 200px; height: 26px; font-size: 12px; }
  .count { min-width: 56px; padding: 0 6px; font-size: 12px; text-align: right; white-space: nowrap; }
  .icon { padding: 0 6px; }
</style>
