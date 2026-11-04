<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { count, dayLabel, dollars, duration, periodLabel, tokens } from '../lib/format';
  import type { ContributionBucket, Granularity, MrOutcomes, Overview } from '../lib/types';

  const WINDOWS = [7, 30, 90, 365];
  let windowDays = $state(30);
  let data = $state<Overview | null>(null);
  let hover = $state<number | null>(null);
  /** État sur GitLab des MR de la période : lu à part, la page n'attend pas le réseau. */
  let outcomes = $state<MrOutcomes | string | null>(null);

  $effect(() => {
    const [w, p] = [windowDays, app.project];
    void app.history.length; // recalcul quand l'historique est relu
    api.stats(w, p).then((o) => (data = o), (e) => app.notify(String(e)));
  });

  $effect(() => {
    const refs = data?.mergeRequestRefs;
    if (!refs) return;
    outcomes = null;
    if (refs.length === 0) return;
    api.mergeRequestOutcomes(refs).then((o) => (outcomes = o), (e) => (outcomes = String(e)));
  });

  // Activité GitLab : tous projets, indépendante de la période et du projet choisis plus haut.
  const GRANULARITIES: [Granularity, string][] = [['day', 'Jour'], ['week', 'Semaine'], ['month', 'Mois'], ['year', 'Année']];
  /** Échelles très différentes (commits contre MR) : un petit graphe par série, pas d'axe commun. */
  const SERIES: { key: Exclude<keyof ContributionBucket, 'startDay'>; label: string; hint: string }[] = [
    { key: 'mrsOpened', label: 'MR ouvertes', hint: 'MR que j’ai ouvertes' },
    { key: 'mrsMerged', label: 'MR mergées', hint: 'Mes MR mergées, par moi ou par n’importe qui' },
    { key: 'commits', label: 'Commits poussés', hint: 'Commits de chaque push : un push forcé après rebase les recompte' },
    { key: 'reviews', label: 'Reviews', hint: 'MR approuvées ; un commentaire seul ne compte pas' },
  ];
  let granularity = $state<Granularity>('week');
  /** Dernier résultat, gardé pendant la lecture suivante : rien ne saute en changeant de regroupement. */
  let activity = $state<ContributionBucket[] | null>(null);
  let activityError = $state<string | null>(null);
  let activityLoading = $state(false);
  let activityHover = $state<number | null>(null);
  let activityRequest = 0;

  $effect(() => {
    const g = granularity;
    if (!app.env?.forge) return;
    const request = ++activityRequest;
    activityLoading = true;
    api.gitlabActivity(g).then(
      (buckets) => {
        if (request !== activityRequest) return;
        [activity, activityError, activityLoading, activityHover] = [buckets, null, false, null];
      },
      (e) => {
        if (request !== activityRequest) return;
        [activityError, activityLoading] = [String(e), false];
      },
    );
  });

  const MW = 240, MH = 64;
  const multiples = $derived.by(() => {
    if (!activity || activity.length === 0) return null;
    const buckets = activity;
    const slot = MW / buckets.length;
    const gap = Math.min(2, slot * 0.25);
    return SERIES.map((s) => {
      const values = buckets.map((b) => b[s.key]);
      const max = Math.max(1, ...values);
      const bars = values.map((v, i) => {
        const h = (v / max) * MH;
        return { x: i * slot + gap / 2, w: Math.max(1, slot - gap), y: MH - h, h };
      });
      return { ...s, total: values.reduce((a, b) => a + b, 0), bars, slot };
    });
  });
  const activityHovered = $derived(activity && activityHover !== null ? activity[activityHover] : null);

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
        <div class="tile" title="Tokens × tarif public de l'API, relevé le {data.pricesAsOf}. Avec un abonnement, ce n'est pas ce que tu paies.">
          <span class="k">Coût estimé</span><span class="v num">{dollars(data.cost.dollars)}</span>
          <span class="s">
            {data.cost.unpricedTokens ? `${tokens(data.cost.unpricedTokens)} tokens sans prix connu` : `tarif API du ${data.pricesAsOf}`}
          </span>
        </div>
        <div class="tile" title="Du premier au dernier message de chaque session">
          <span class="k">Durée médiane</span>
          <span class="v num">{data.medianDurationMs === null ? '—' : duration(data.medianDurationMs)}</span>
        </div>
        <div class="tile"><span class="k">Appels d'outils</span><span class="v num">{count(data.toolCalls)}</span></div>
        <div class="tile" title="MR liées aux sessions (pr-link), état lu sur {app.forgeName}">
          <span class="k">MR mergées</span>
          {#if data.mergeRequestRefs.length === 0}
            <span class="v num">—</span><span class="s">aucune MR liée à une session</span>
          {:else if outcomes === null}
            <span class="v num">…</span><span class="s num">{data.mergeRequestRefs.length} MR à lire sur {app.forgeName}</span>
          {:else if typeof outcomes === 'string'}
            <span class="v num">?</span><span class="s" title={outcomes}>{app.forgeName} illisible</span>
          {:else}
            <span class="v num">{Math.round((outcomes.merged / outcomes.total) * 100)} %</span>
            <span class="s num">
              {outcomes.merged}/{outcomes.total} · {outcomes.opened} ouverte{outcomes.opened > 1 ? 's' : ''}{outcomes.closed
                ? ` · ${outcomes.closed} fermée${outcomes.closed > 1 ? 's' : ''}`
                : ''}{outcomes.unknown ? ` · ${outcomes.unknown} illisible${outcomes.unknown > 1 ? 's' : ''}` : ''}
            </span>
          {/if}
        </div>
      </div>

      <div class="panel">
        <h2 class="section-title">Agents lancés dans Robiade</h2>
        {#if data.agents.agents === 0}
          <p class="muted">Aucun agent Robiade sur la période : ces mesures viennent des hooks, pas des transcripts.</p>
        {:else}
          <div class="tiles">
            <div class="tile"><span class="k">Agents</span><span class="v num">{count(data.agents.agents)}</span></div>
            <div class="tile" title="Agents ayant eu au moins un tour terminé en erreur (StopFailure)">
              <span class="k">Taux d'échec</span>
              <span class="v num">{Math.round((data.agents.failed / data.agents.agents) * 100)} %</span>
              <span class="s num">{data.agents.failed} agent{data.agents.failed > 1 ? 's' : ''}</span>
            </div>
            <div class="tile" title="Temps où un agent attendait une permission ou la lecture de son tour : le goulot, c'est moi">
              <span class="k">T'ont attendu</span><span class="v num">{duration(data.agents.waitedMs)}</span>
              <span class="s num">dont {duration(data.agents.permissionMs)} de permission</span>
            </div>
          </div>
        {/if}
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
                  <td class="num right muted">{dollars(p.cost.dollars)}</td>
                  <td class="num right muted">{p.sessions} s.</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <div class="panel">
          <h2 class="section-title">Par modèle</h2>
          <table>
            <tbody>
              {#each data.models as m (m.model)}
                <tr>
                  <td class="name mono">{m.model}</td>
                  <td class="num right">{tokens(m.tokens.total)}</td>
                  <td class="num right muted">{m.cost === null ? 'sans prix' : dollars(m.cost)}</td>
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

      {#if app.env?.forge}
      <div class="panel">
        <div class="panel-head">
          <h2 class="section-title">Activité {app.forgeName}</h2>
          <span class="muted small">tous projets{activityLoading ? ' · lecture…' : ''}</span>
          <span class="grow"></span>
          <div class="seg" role="radiogroup" aria-label="Regroupement de l'activité {app.forgeName}">
            {#each GRANULARITIES as [g, label] (g)}
              <button role="radio" aria-checked={granularity === g} class:on={granularity === g} onclick={() => (granularity = g)}>
                {label}
              </button>
            {/each}
          </div>
        </div>
        {#if activityError}
          <p class="muted small" title={activityError}>{app.forgeName} illisible : {activityError}</p>
        {/if}
        {#if !activity}
          {#if !activityError}<p class="muted small">Lecture des événements {app.forgeName}…</p>{/if}
        {:else if multiples}
          <div class="multiples">
            {#each multiples as m (m.key)}
              <div class="multiple">
                <div class="mhead">
                  <span class="k" title={m.hint}>{m.label}</span>
                  <span class="num">{activityHovered ? count(activityHovered[m.key]) : count(m.total)}</span>
                </div>
                <svg viewBox="0 0 {MW} {MH}" preserveAspectRatio="none" role="img" aria-label="{m.label} : {m.total} sur la période">
                  <line x1="0" x2={MW} y1={MH} y2={MH} class="grid" />
                  {#each m.bars as b, i (i)}
                    <rect x={i * m.slot} y="0" width={m.slot} height={MH} class="hit" role="presentation"
                      onmouseenter={() => (activityHover = i)} onmouseleave={() => (activityHover = null)} />
                    <path d={barPath(b.x, b.y, b.w, b.h)} class="bar" class:dim={activityHover !== null && activityHover !== i}
                      pointer-events="none" />
                  {/each}
                </svg>
              </div>
            {/each}
          </div>
          <div class="xlabels static">
            {#if activityHovered}
              <span><strong>{periodLabel(activityHovered.startDay, granularity)}</strong></span>
              <span class="muted">les totaux affichent cette période</span>
            {:else}
              <span>{periodLabel(activity[0]?.startDay ?? 0, granularity)}</span>
              <span>{periodLabel(activity.at(-1)?.startDay ?? 0, granularity)}</span>
            {/if}
          </div>
        {/if}
      </div>
      {/if}
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
  .tiles { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 12px; }
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
  .two { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 12px; }
  table { width: 100%; border-collapse: collapse; }
  td { padding: 5px 0; }
  .name { width: 34%; max-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding-right: 10px; }
  .barcell { width: 46%; }
  .hbar { display: block; height: 8px; border-radius: 0 4px 4px 0; background: var(--chart); min-width: 2px; }
  .right { text-align: right; padding-left: 10px; white-space: nowrap; }
  .panel-head { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; }
  .panel-head .section-title { margin: 0; }
  .grow { flex: 1; }
  .small { font-size: 12px; margin: 0; }
  .multiples { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 12px 20px; }
  .multiple { display: flex; flex-direction: column; gap: 4px; }
  .mhead { display: flex; justify-content: space-between; align-items: baseline; font-size: 13px; }
  .mhead .num { font-weight: 600; }
  .multiple svg { height: 64px; }
  .xlabels.static { margin-top: 8px; }
</style>
