<script lang="ts">
  import { mrNumber, mrRefOf } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import type { MrState } from '../lib/types';

  /**
   * Sans `state`, l'état lu sur GitLab pour cette URL ; neutre tant qu'il n'est pas connu.
   * Un clic lit la MR dans le panneau ; `external` (dans une modale, qui le cacherait) et ⌘+clic
   * l'ouvrent dans le navigateur. Avec `title`, il suit le numéro, coupé par la fin.
   */
  let {
    url,
    state: given,
    title,
    external = false,
  }: { url: string; state?: MrState; title?: string; external?: boolean } = $props();
  const state = $derived(given ?? app.mrStates[url] ?? 'unknown');

  const LABELS = { unknown: 'MR liée', draft: 'Draft', opened: 'Ouverte', merged: 'Mergée', closed: 'Fermée' };
  const number = $derived(mrNumber(url));
  const ref = $derived(external ? null : mrRefOf(url));
</script>

<button
  class="mr {state}"
  title="{title ? `!${number} ${title}\n` : ''}{LABELS[state]} — {ref ? 'lire (⌘+clic : GitLab)' : 'ouvrir dans GitLab'}"
  onclick={(e) => {
    e.stopPropagation();
    if (ref) app.read({ kind: 'mr', ...ref, webUrl: url }, e);
    else app.openUrl(url);
  }}
>
  <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
    {#if state === 'merged'}
      <circle cx="4" cy="3.5" r="1.8" /><circle cx="12" cy="11" r="1.8" /><path d="M4 5.3v7.2M4 7c0 3 4 4 6.2 4" />
    {:else if state === 'closed'}
      <circle cx="4" cy="3.5" r="1.8" /><circle cx="4" cy="12.5" r="1.8" /><path d="M4 5.3v5.4M10 3l4 4M14 3l-4 4" />
    {:else}
      <circle cx="4" cy="3.5" r="1.8" /><circle cx="4" cy="12.5" r="1.8" /><circle cx="12" cy="12.5" r="1.8" />
      <path d="M4 5.3v5.4M12 10.7V6.5c0-1.4-1-2.5-2.5-2.5H7" />
    {/if}
  </svg>
  {#if number}<span class="num">!{number}</span>{/if}
  {#if title}<span class="title">{title}</span>{/if}
</button>

<style>
  .mr {
    --c: var(--ink-2);
    display: inline-flex; align-items: center; gap: 4px;
    height: 20px; padding: 0 6px; border-radius: 10px;
    border: 1px solid color-mix(in srgb, var(--c) 35%, transparent);
    background: color-mix(in srgb, var(--c) 10%, transparent);
    color: var(--c); font: 11.5px var(--mono);
  }
  .mr:has(.title) { max-width: 240px; min-width: 0; overflow: hidden; }
  .title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; font-family: var(--font); }
  svg, .num { flex: none; }
  .mr:hover { background: color-mix(in srgb, var(--c) 18%, transparent); }
  .draft { --c: var(--mr-draft); }
  .opened { --c: var(--mr-open); }
  .merged { --c: var(--mr-merged); }
  .closed { --c: var(--mr-closed); }
  svg { fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; }
  .unknown svg circle { fill: none; }
</style>
