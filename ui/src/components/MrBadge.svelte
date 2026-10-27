<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { mrNumber } from '../lib/format';

  /** `state` viendra de l'API GitLab (v2). En attendant : état inconnu, rendu neutre. */
  let { url, state = 'unknown' }: { url: string; state?: 'unknown' | 'draft' | 'opened' | 'merged' | 'closed' } =
    $props();

  const LABELS = { unknown: 'MR liée', draft: 'Draft', opened: 'Ouverte', merged: 'Mergée', closed: 'Fermée' };
  const number = $derived(mrNumber(url));
</script>

<button
  class="mr {state}"
  title="{LABELS[state]} — ouvrir dans GitLab"
  onclick={(e) => {
    e.stopPropagation();
    void openUrl(url);
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
  .mr:hover { background: color-mix(in srgb, var(--c) 18%, transparent); }
  .draft { --c: var(--mr-draft); }
  .opened { --c: var(--mr-open); }
  .merged { --c: var(--mr-merged); }
  .closed { --c: var(--mr-closed); }
  svg { fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; }
  .unknown svg circle { fill: none; }
</style>
