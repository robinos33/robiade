<script lang="ts">
  import type { Gauges } from '../lib/types';
  import { time, tokens } from '../lib/format';

  /** Barre native qui remplace la statusline de Claude Code dans le terminal intégré. */
  let { gauges }: { gauges: Gauges } = $props();

  const meters = $derived(
    [
      gauges.context && {
        label: 'Contexte',
        gauge: gauges.context,
        detail:
          gauges.contextTokens !== null && gauges.contextWindow
            ? `${tokens(gauges.contextTokens)} / ${tokens(gauges.contextWindow)}`
            : null,
      },
      gauges.fiveHour && {
        label: '5 h',
        gauge: gauges.fiveHour,
        detail: gauges.fiveHourResetsAt ? `fin ${time(gauges.fiveHourResetsAt)}` : null,
      },
      gauges.week && { label: 'Semaine', gauge: gauges.week, detail: null },
    ].filter((m) => !!m),
  );
</script>

{#if gauges.model || meters.length > 0}
  <div class="bar" role="status" aria-label="Statusline de la session">
    {#if gauges.model}<span class="model">{gauges.model}</span>{/if}
    {#each meters as m (m.label)}
      <span class="meter {m.gauge.level}" title="{m.label} : {m.gauge.percent} %">
        <span class="label">{m.label}</span>
        <span class="track"><span class="fill" style:width="{m.gauge.percent}%"></span></span>
        <span class="num pct">{m.gauge.percent} %</span>
        {#if m.detail}<span class="num muted">{m.detail}</span>{/if}
      </span>
    {/each}
  </div>
{/if}

<style>
  .bar {
    display: flex; align-items: center; gap: 18px; flex: none; height: 30px; padding: 0 12px;
    border-bottom: 1px solid var(--line); background: var(--bg); font-size: 11.5px; color: var(--ink-2);
    overflow: hidden; white-space: nowrap;
  }
  .model { font-weight: 600; color: var(--ink); }
  .meter { --c: var(--ok); display: inline-flex; align-items: center; gap: 6px; }
  .meter.watch { --c: var(--attention); }
  .meter.critical { --c: var(--danger); }
  .label { color: var(--ink-3); }
  .track { width: 56px; height: 5px; border-radius: 3px; background: var(--line-strong); overflow: hidden; }
  .fill { display: block; height: 100%; border-radius: inherit; background: var(--c); transition: width 0.4s ease; }
  .pct { color: var(--ink); font-weight: 550; }
  /* Le niveau ne repose pas sur la couleur seule : à partir de « watch », le pourcentage se colore et s'épaissit. */
  .watch .pct, .critical .pct { color: var(--c); font-weight: 700; }
  @media (prefers-reduced-motion: reduce) { .fill { transition: none; } }
</style>
