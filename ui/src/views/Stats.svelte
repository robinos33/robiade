<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { count, dayLabel, tokens } from '../lib/format';
  import type { Overview } from '../lib/types';

  const WINDOWS = [7, 30, 90, 365];
  let windowDays = $state(30);
  let data = $state<Overview | null>(null);
  let hover = $state<number | null>(null);

  $effect(() => {
    const [w, p] = [windowDays, app.project];
    void app.history.length; // recalcul quand l'historique est relu
    api.stats(w, p).then((o) => (data = o), (e) => app.notify(String(e)));
  });

  // Graphe : une seule série → une teinte, pas de légende, survol par barre.
  const W = 760, H = 180, PAD_B = 22, PAD_T = 8;
  const chart = $derived.by(() => {
    if (!data) return null;
    const days = data.days;
    const max = Math.max(1, ...days.map((d) => d.sessions));
    const slot = W / days.length;
    const gap = Math.min(2, slot * 0.25);
    const bars = days.map((d, i) => {
      const h = (d.sessions / max) * (H - PAD_B - PAD_T);
      return { ...d, x: i * slot + gap / 2, w: Math.max(1, slot - gap), y: H - PAD_B - h, h };
    });
    const ticks = [0, Math.ceil(max / 2), max].filter((v, i, a) => a.indexOf(v) === i);
    return { bars, max, ticks, slot };
  });
  const hovered = $derived(chart && hover !== null ? chart.bars[hover] : null);
  const maxProjectTokens = $derived(Math.max(1, ...(data?.projects.map((p) => p.tokens.total) ?? [1])));
  const maxTool = $derived(Math.max(1, ...(data?.tools.map((t) => t[1]) ?? [1])));

  // Une barre de 0 doit rester lisible comme « rien », sans disparaître du rythme.
  function barPath(x: number, y: number, w: number, h: number): string {
    const r = Math.min(4, w / 2, h);
    if (h <= 0) return '';
    return `M${x},${y + h}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + h}Z`;
  }
</script>

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Statistiques</h1>
    <span class="muted">{app.project ? app.projects.find((p) => p.root === app.project)?.name : 'Tous les projets'}</span>
    <span class="spacer"></span>
    <div class="seg" role="radiogroup" aria-label="Période">
      {#each WINDOWS as w (w)}
        <button role="radio" aria-checked={windowDays === w} class:on={windowDays === w} onclick={() => (windowDays = w)}>
          {w === 365 ? '1 an' : `${w} j`}
        </button>
      {/each}
    </div>
  </header>

  <div class="scroll">
    {#if !data}
      <p class="muted">Calcul…</p>
    {:else}
      <div class="tiles">
        <div class="tile"><span class="k">Sessions</span><span class="v num">{count(data.sessions)}</span></div>
        <div class="tile">
          <span class="k">Tokens</span><span class="v num">{tokens(data.tokens.total)}</span>
          <span class="s num">{tokens(data.tokens.output)} en sortie · {tokens(data.tokens.cacheRead)} lus en cache</span>
        </div>
        <div class="tile"><span class="k">Appels d'outils</span><span class="v num">{count(data.toolCalls)}</span></div>
        <div class="tile"><span class="k">Sessions avec MR</span><span class="v num">{count(data.mergeRequests)}</span></div>
      </div>

      <div class="panel">
        <h2 class="section-title">Sessions par jour</h2>
        {#if chart}
          <div class="chart">
            <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img" aria-label="Sessions par jour">
              {#each chart.ticks as t (t)}
                {@const y = H - PAD_B - (t / chart.max) * (H - PAD_B - PAD_T)}
                <line x1="0" x2={W} y1={y} y2={y} class="grid" />
              {/each}
              {#each chart.bars as b, i (b.day)}
                <rect x={i * chart.slot} y="0" width={chart.slot} height={H} class="hit" role="presentation"
                  onmouseenter={() => (hover = i)} onmouseleave={() => (hover = null)} />
                <path d={barPath(b.x, b.y, b.w, b.h)} class="bar" class:dim={hover !== null && hover !== i} pointer-events="none" />
              {/each}
            </svg>
            <div class="axis">
              {#each chart.ticks as t (t)}
                <span class="num" style="bottom: {PAD_B + (t / chart.max) * (H - PAD_B - PAD_T) - 7}px">{t}</span>
              {/each}
            </div>
            <div class="xlabels">
              <span>{dayLabel(chart.bars[0]?.day ?? 0)}</span>
              <span>Aujourd'hui</span>
            </div>
            {#if hovered && hover !== null}
              <div class="tip" style="left: {((hover + 0.5) / chart.bars.length) * 100}%">
                <strong>{dayLabel(hovered.day)}</strong>
                <span class="num">{hovered.sessions} session{hovered.sessions > 1 ? 's' : ''}</span>
                <span class="num muted">{tokens(hovered.tokens)} tokens</span>
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <div class="two">
        <div class="panel">
          <h2 class="section-title">Tokens par projet</h2>
          <table>
            <tbody>
              {#each data.projects.slice(0, 10) as p (p.projectPath)}
                <tr>
                  <td class="name" title={p.projectPath}>{p.projectName}</td>
                  <td class="barcell"><span class="hbar" style="width: {(p.tokens.total / maxProjectTokens) * 100}%"></span></td>
                  <td class="num right">{tokens(p.tokens.total)}</td>
                  <td class="num right muted">{p.sessions} s.</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <div class="panel">
          <h2 class="section-title">Outils les plus appelés</h2>
          <table>
            <tbody>
              {#each data.tools as [name, n] (name)}
                <tr>
                  <td class="name mono">{name}</td>
                  <td class="barcell"><span class="hbar" style="width: {(n / maxTool) * 100}%"></span></td>
                  <td class="num right">{count(n)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </div>

      <div class="panel later">
        <h2 class="section-title">Activité GitLab</h2>
        <p class="muted">MR ouvertes et mergées, commits, reviews, par jour / semaine / mois / année. Prévu avec le module Forge (v2).</p>
      </div>
    {/if}
  </div>
</section>

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .scroll { overflow-y: auto; padding: 20px 24px 40px; display: flex; flex-direction: column; gap: 18px; max-width: 1200px; }
  .seg { display: inline-flex; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); overflow: hidden; }
  .seg button { border: 0; background: none; padding: 0 10px; height: 26px; color: var(--ink-2); font-size: 12px; }
  .seg button + button { border-left: 1px solid var(--line-strong); }
  .seg button.on { background: var(--bg-active); color: var(--ink); font-weight: 600; }
  .tiles { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; }
  .tile, .panel { background: var(--bg-raised); border: 1px solid var(--line); border-radius: var(--radius); padding: 14px 16px; }
  .tile { display: flex; flex-direction: column; gap: 2px; }
  .k { color: var(--ink-3); font-size: 12px; }
  .v { font-size: 26px; font-weight: 640; letter-spacing: -0.02em; }
  .s { color: var(--ink-3); font-size: 11.5px; }
  .chart { position: relative; padding-left: 28px; }
  svg { width: 100%; height: 180px; display: block; overflow: visible; }
  .grid { stroke: var(--line); stroke-width: 1; vector-effect: non-scaling-stroke; }
  .bar { fill: var(--chart); transition: opacity 120ms; }
  .bar.dim { opacity: 0.45; }
  .hit { fill: transparent; }
  .axis { position: absolute; left: 0; top: 0; bottom: 0; width: 24px; }
  .axis span { position: absolute; right: 0; font-size: 11px; color: var(--ink-3); }
  .xlabels { display: flex; justify-content: space-between; font-size: 11px; color: var(--ink-3); margin-top: -16px; }
  .tip {
    position: absolute; top: -6px; transform: translate(-50%, -100%); pointer-events: none;
    display: flex; flex-direction: column; gap: 1px; padding: 7px 10px; white-space: nowrap;
    background: var(--bg-raised); border-radius: 6px; box-shadow: var(--shadow); font-size: 12px;
  }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  table { width: 100%; border-collapse: collapse; }
  td { padding: 5px 0; }
  .name { width: 34%; max-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding-right: 10px; }
  .barcell { width: 46%; }
  .hbar { display: block; height: 8px; border-radius: 0 4px 4px 0; background: var(--chart); min-width: 2px; }
  .right { text-align: right; padding-left: 10px; white-space: nowrap; }
  .later p { margin: 0; }
</style>
