<script lang="ts">
  import { untrack } from 'svelte';
  import { api } from '../lib/api';
  import type { DbCell, DbColumn, DbCursor, DbFilter, DbRelation, DbSort } from '../lib/types';
  import DbTable from './DbTable.svelte';

  let { connectionId, relation }: { connectionId: string; relation: DbRelation } = $props();

  let columns = $state<DbColumn[]>([]);
  let rows = $state<(DbCell | null)[][]>([]);
  let next = $state<DbCursor | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let sort = $state<DbSort | null>(null);
  let filters = $state<DbFilter[]>([]);

  /** Chaque rechargement a son numéro : une réponse tardive d'une requête dépassée est ignorée. */
  let generation = 0;

  async function load(reset: boolean) {
    const mine = reset ? ++generation : generation;
    if (!reset && (loading || !next)) return;
    loading = true;
    error = null;
    try {
      const cursor: DbCursor = reset ? { kind: 'start' } : next!;
      const page = await api.dbPage(connectionId, relation.schema, relation.name, { sort, filters, cursor });
      if (mine !== generation) return;
      columns = page.columns;
      if (!columns.some((c) => c.name === filterColumn)) filterColumn = columns[0]?.name ?? '';
      rows = reset ? page.rows : [...rows, ...page.rows];
      next = page.next;
    } catch (e) {
      if (mine === generation) error = String(e);
    } finally {
      if (mine === generation) loading = false;
    }
  }

  // Nouvelle table, nouveau tri ou nouveaux filtres : on repart de la première page.
  $effect(() => {
    void [relation.schema, relation.name, connectionId, sort, filters];
    untrack(() => {
      rows = [];
      next = null;
      void load(true);
    });
  });

  /** Aucun tri → croissant → décroissant → aucun. */
  function toggleSort(column: string) {
    if (sort?.column !== column) sort = { column, descending: false };
    else if (!sort.descending) sort = { column, descending: true };
    else sort = null;
  }

  // Ajout d'un filtre : colonne, opération, valeur.
  let filterColumn = $state('');
  let filterOp = $state<DbFilter['op']>('equals');
  let filterValue = $state('');
  const needsValue = $derived(filterOp === 'equals' || filterOp === 'contains');

  function addFilter(e: SubmitEvent) {
    e.preventDefault();
    const column = filterColumn;
    if (!column || (filterOp === 'contains' && !filterValue)) return;
    const filter: DbFilter =
      filterOp === 'equals' || filterOp === 'contains' ? { column, op: filterOp, value: filterValue } : { column, op: filterOp };
    filters = [...filters, filter];
    filterValue = '';
  }

  const OPS: Record<DbFilter['op'], string> = { equals: '=', contains: 'contient', isNull: 'est NULL', isNotNull: "n'est pas NULL" };

</script>

<div class="grid-wrap">
  <form class="filters" onsubmit={addFilter}>
    <select bind:value={filterColumn} aria-label="Colonne">
      {#each columns as c (c.name)}<option value={c.name}>{c.name}</option>{/each}
    </select>
    <select bind:value={filterOp} aria-label="Opération">
      {#each Object.entries(OPS) as [op, label] (op)}<option value={op}>{label}</option>{/each}
    </select>
    {#if needsValue}<input class="value" bind:value={filterValue} placeholder="valeur" spellcheck="false" />{/if}
    <button class="btn small" type="submit" disabled={filters.length >= 10}>Filtrer</button>
    {#each filters as f, i (i)}
      <span class="chip mono">
        {f.column} {OPS[f.op]}{'value' in f ? ` « ${f.value} »` : ''}
        <button type="button" aria-label="Retirer le filtre" onclick={() => (filters = filters.filter((_, j) => j !== i))}>×</button>
      </span>
    {/each}
    <span class="spacer"></span>
    <span class="muted count">{rows.length}{next ? '+' : ''} ligne{rows.length > 1 ? 's' : ''}{loading ? ' · lecture…' : ''}</span>
  </form>

  {#if error}<p class="error-text">{error}</p>{/if}

  <DbTable
    {columns}
    {rows}
    {sort}
    onsort={toggleSort}
    onnearend={() => void load(false)}
    empty={!loading && !error ? 'Aucune ligne.' : null}
  />
</div>

<style>
  .grid-wrap { display: flex; flex-direction: column; min-height: 0; min-width: 0; flex: 1; }
  .filters { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; padding: 8px 12px; border-bottom: 1px solid var(--line); }
  .filters select, .filters .value {
    height: 26px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--bg-raised);
    color: var(--ink); font-size: 12px; padding: 0 6px;
  }
  .filters .value { width: 180px; font-family: var(--mono); }
  .chip {
    display: inline-flex; align-items: center; gap: 4px; height: 22px; padding: 0 4px 0 8px; border-radius: 11px;
    background: var(--accent-soft); font-size: 11.5px;
  }
  .chip button { border: 0; background: none; color: var(--ink-2); cursor: pointer; font-size: 13px; }
  .count { font-size: 12px; }
  .spacer { flex: 1; }
  .small { height: 24px; padding: 0 8px; font-size: 12px; }
  .error-text { color: var(--danger); margin: 8px 12px; font-size: 12.5px; overflow-wrap: anywhere; }
</style>
