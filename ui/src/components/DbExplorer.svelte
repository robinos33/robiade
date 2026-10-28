<script lang="ts">
  import { untrack } from 'svelte';
  import { api } from '../lib/api';
  import type { DbConnection, DbOverview, DbRelation, DbRelationKind } from '../lib/types';
  import DbConsole from './DbConsole.svelte';
  import DbGrid from './DbGrid.svelte';
  import StageBadge from './StageBadge.svelte';

  let { connection, onclose }: { connection: DbConnection; onclose: () => void } = $props();

  let overview = $state<DbOverview | null>(null);
  let error = $state<string | null>(null);
  let opening = $state(false);
  let selected = $state<DbRelation | null>(null);
  let search = $state('');
  /** Tables ou console ; la console reste montée pour garder son texte et son résultat. */
  let tab = $state<'tables' | 'console'>('tables');
  /** Montée à la première ouverture seulement : l'éditeur se mesure mal tant qu'il est caché. */
  let consoleOpened = $state(false);
  $effect(() => {
    if (tab === 'console') consoleOpened = true;
  });

  /** Ouvre la session, dans `database` ou la base de la connexion ; elle remplace la précédente. */
  async function open(database: string | null) {
    opening = true;
    error = null;
    selected = null;
    try {
      overview = await api.openDb(connection.id, database);
    } catch (e) {
      error = String(e);
    } finally {
      opening = false;
    }
  }

  // Une session par connexion ouverte, fermée quand on quitte l'explorateur.
  $effect(() => {
    const id = connection.id;
    untrack(() => void open(null));
    return () => void api.closeDb(id);
  });

  const KIND: Record<DbRelationKind, string> = { table: '', view: 'vue', materializedView: 'vue mat.', foreign: 'externe' };

  /** Relations filtrées par la recherche, groupées par schéma. */
  const schemas = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const groups = new Map<string, DbRelation[]>();
    for (const r of overview?.relations ?? []) {
      if (q && !`${r.schema}.${r.name}`.toLowerCase().includes(q)) continue;
      groups.set(r.schema, [...(groups.get(r.schema) ?? []), r]);
    }
    return [...groups];
  });

  const rowsLabel = (n: number | null) =>
    n === null ? '' : n >= 1_000_000 ? `~${(n / 1_000_000).toFixed(1)} M` : n >= 1000 ? `~${Math.round(n / 1000)} k` : `~${n}`;

  const prod = $derived(connection.stage === 'prod');
</script>

<section class="explorer" class:prod>
  <header class="page-head" data-tauri-drag-region>
    <button class="btn small" onclick={onclose}>← Connexions</button>
    <StageBadge stage={connection.stage} />
    <h1 class="ellipsis">{connection.name}</h1>
    {#if overview}
      <select
        class="db"
        aria-label="Base"
        value={overview.database}
        disabled={opening}
        onchange={(e) => void open(e.currentTarget.value)}
      >
        {#each overview.databases as d (d)}<option value={d}>{d}</option>{/each}
      </select>
      {#if overview.readOnly}<span class="tag">lecture seule (vérifiée sur le serveur)</span>{/if}
      <div class="tabs" role="tablist">
        <button role="tab" class:on={tab === 'tables'} aria-selected={tab === 'tables'} onclick={() => (tab = 'tables')}>Tables</button>
        <button role="tab" class:on={tab === 'console'} aria-selected={tab === 'console'} onclick={() => (tab = 'console')}>Console</button>
      </div>
    {/if}
    {#if opening}<span class="muted">connexion…</span>{/if}
  </header>

  {#if prod}
    <!-- Impossible à manquer : on lit la production. -->
    <div class="prod-banner">
      <StageBadge stage="prod" />
      <strong>PRODUCTION</strong> · {connection.name}{overview ? ` · base ${overview.database}` : ''} · lecture seule
    </div>
  {/if}

  {#if error}<p class="error-text">{error}</p>{/if}

  <div class="panes">
    <nav class="tree">
      <input class="search" bind:value={search} placeholder="Filtrer les tables" spellcheck="false" />
      <div class="list">
        {#each schemas as [schema, relations] (schema)}
          <h3 class="schema mono">{schema}</h3>
          {#each relations as r (r.name)}
            <button
              class="relation"
              class:active={selected?.schema === r.schema && selected?.name === r.name}
              onclick={() => ((selected = r), (tab = 'tables'))}
              title="{r.schema}.{r.name}"
            >
              <span class="ellipsis mono">{r.name}</span>
              {#if KIND[r.kind]}<span class="kind">{KIND[r.kind]}</span>{/if}
              <span class="spacer"></span>
              <span class="rows muted">{rowsLabel(r.estimatedRows)}</span>
            </button>
          {/each}
        {/each}
        {#if overview && schemas.length === 0}<p class="muted none">Aucune table lisible.</p>{/if}
      </div>
    </nav>

    {#if overview && consoleOpened}
      <div class="main" hidden={tab !== 'console'}>
        <DbConsole {connection} readOnly={overview.readOnly} />
      </div>
    {/if}
    <div class="main" hidden={tab !== 'tables'}>
      {#if selected}
        {#key `${overview?.database}/${selected.schema}.${selected.name}`}
          <DbGrid connectionId={connection.id} relation={selected} />
        {/key}
      {:else}
        <p class="muted hint">Choisis une table à gauche.</p>
      {/if}
    </div>
  </div>
</section>

<style>
  .explorer { display: flex; flex-direction: column; min-height: 0; flex: 1; border-top: 3px solid transparent; }
  .explorer.prod { border-top-color: var(--danger); }
  .small { height: 24px; padding: 0 8px; font-size: 12px; }
  h1 { margin: 0; font-size: 15px; max-width: 30%; }
  .prod h1 { color: var(--danger); }
  .db {
    height: 26px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--bg-raised);
    color: var(--ink); font: 12px var(--mono); padding: 0 6px;
  }
  .tag { flex: none; font-size: 11px; padding: 0 6px; border-radius: 8px; border: 1px solid var(--line-strong); color: var(--ink-2); }
  .prod-banner {
    display: flex; align-items: center; gap: 8px; padding: 6px 16px; background: var(--danger); color: #fff; font-size: 12.5px;
  }
  .error-text { color: var(--danger); margin: 8px 16px; font-size: 12.5px; overflow-wrap: anywhere; }
  .panes { display: flex; flex: 1; min-height: 0; }
  .tree { width: 260px; flex: none; border-right: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
  .search {
    margin: 8px; height: 26px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm);
    background: var(--bg-raised); color: var(--ink); font-size: 12px; padding: 0 8px;
  }
  .list { overflow-y: auto; flex: 1; padding-bottom: 12px; }
  .schema { margin: 10px 12px 4px; font-size: 11px; font-weight: 600; color: var(--ink-3); }
  .relation {
    display: flex; align-items: center; gap: 6px; width: 100%; height: 26px; padding: 0 12px 0 20px; border: 0;
    background: none; color: var(--ink); font-size: 12px; text-align: left; cursor: pointer; min-width: 0;
  }
  .relation:hover { background: var(--bg-hover); }
  .relation.active { background: var(--bg-active); }
  .kind { font-size: 10.5px; color: var(--info); flex: none; }
  .rows { font-size: 11px; flex: none; }
  .spacer { flex: 1; }
  .none { padding: 0 12px; }
  .main { flex: 1; display: flex; min-width: 0; min-height: 0; }
  .main[hidden] { display: none; }
  .tabs { display: flex; gap: 2px; margin-left: 8px; }
  .tabs button {
    height: 26px; padding: 0 10px; border: 0; border-radius: var(--radius-sm); background: none; color: var(--ink-3);
    font-size: 12.5px; cursor: pointer;
  }
  .tabs button:hover { color: var(--ink); background: var(--bg-hover); }
  .tabs button.on { color: var(--ink); background: var(--bg-active); font-weight: 550; }
  .hint { padding: 16px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
