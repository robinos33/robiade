<script lang="ts">
  import { untrack } from 'svelte';
  import { api } from '../lib/api';
  import type { DbCell, DbColumn, DbCursor, DbFilter, DbRelation, DbSort } from '../lib/types';

  let { connectionId, relation }: { connectionId: string; relation: DbRelation } = $props();

  /** Hauteur fixe d'une ligne : la grille ne rend que les lignes visibles, calculées au défilement. */
  const ROW_H = 26;
  const OVERSCAN = 10;
  const COL_W = 200;

  let columns = $state<DbColumn[]>([]);
  let rows = $state<(DbCell | null)[][]>([]);
  let next = $state<DbCursor | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let sort = $state<DbSort | null>(null);
  let filters = $state<DbFilter[]>([]);
  /** Cellule ouverte dans le panneau de détail. */
  let detail = $state<{ column: DbColumn; cell: DbCell | null } | null>(null);

  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewport = $state(600);

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
      detail = null;
      rows = [];
      next = null;
      if (scroller) scroller.scrollTop = 0;
      void load(true);
    });
  });

  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN));
  const last = $derived(Math.min(rows.length, Math.ceil((scrollTop + viewport) / ROW_H) + OVERSCAN));
  const template = $derived(`56px repeat(${columns.length}, ${COL_W}px)`);

  function onscroll() {
    if (!scroller) return;
    scrollTop = scroller.scrollTop;
    viewport = scroller.clientHeight;
    // La suite est demandée un peu avant d'atteindre le bas.
    if (next && !loading && scrollTop + viewport > rows.length * ROW_H - 20 * ROW_H) void load(false);
  }

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

  /** Un JSON se lit indenté dans le détail ; tout le reste tel quel. Toujours rendu en texte. */
  function pretty(column: DbColumn, cell: DbCell): string {
    if (!/^jsonb?$/.test(column.dataType) || cell.truncated) return cell.text;
    try {
      return JSON.stringify(JSON.parse(cell.text), null, 2);
    } catch {
      return cell.text;
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      /* presse-papiers refusé : rien à faire */
    }
  }
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

  <div class="body">
    <div class="scroller" bind:this={scroller} {onscroll} bind:clientHeight={viewport}>
      <div class="row head" style:grid-template-columns={template}>
        <span class="cell index"></span>
        {#each columns as c (c.name)}
          <button class="cell th" onclick={() => toggleSort(c.name)} title={c.dataType}>
            <span class="ellipsis">{c.primaryKey ? '🔑 ' : ''}{c.name}</span>
            <span class="type ellipsis">{c.dataType}</span>
            {#if sort?.column === c.name}<span class="arrow">{sort.descending ? '↓' : '↑'}</span>{/if}
          </button>
        {/each}
      </div>
      <div class="rows" style:height="{rows.length * ROW_H}px" style:width="{56 + columns.length * COL_W}px">
        {#each rows.slice(first, last) as row, k (first + k)}
          {@const i = first + k}
          <div class="row" style:grid-template-columns={template} style:top="{i * ROW_H}px">
            <span class="cell index mono">{i + 1}</span>
            {#each row as cell, j (j)}
              <button
                class="cell mono"
                class:null={cell === null}
                class:active={detail?.cell === cell && detail?.column === columns[j]}
                onclick={() => (detail = { column: columns[j]!, cell })}
              >{cell === null ? 'NULL' : cell.text}</button>
            {/each}
          </div>
        {/each}
      </div>
      {#if !loading && rows.length === 0 && !error}<p class="muted empty">Aucune ligne.</p>{/if}
    </div>

    {#if detail}
      {@const d = detail}
      <aside class="detail">
        <header>
          <strong class="ellipsis">{d.column.name}</strong>
          <span class="muted mono type">{d.column.dataType}</span>
          <span class="spacer"></span>
          {#if d.cell}<button class="btn small" onclick={() => void copy(d.cell!.text)}>Copier</button>{/if}
          <button class="btn small" aria-label="Fermer" onclick={() => (detail = null)}>×</button>
        </header>
        {#if d.cell === null}
          <p class="muted mono">NULL</p>
        {:else}
          {#if d.cell.truncated}<p class="note">Valeur tronquée à 10 000 caractères.</p>{/if}
          {#if d.cell.text === ''}<p class="muted">(chaîne vide)</p>{/if}
          <pre class="mono value-full">{pretty(d.column, d.cell)}</pre>
        {/if}
      </aside>
    {/if}
  </div>
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
  .body { display: flex; flex: 1; min-height: 0; }
  .scroller { flex: 1; overflow: auto; position: relative; min-width: 0; }
  .rows { position: relative; }
  .row { display: grid; height: 26px; position: absolute; left: 0; }
  .head { position: sticky; top: 0; z-index: 2; height: 34px; background: var(--bg-sunken); }
  .cell {
    display: flex; align-items: center; min-width: 0; padding: 0 8px; border: 0; border-right: 1px solid var(--line);
    border-bottom: 1px solid var(--line); background: none; color: var(--ink); text-align: left; font-size: 12px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; cursor: default;
  }
  .row:not(.head) .cell { display: block; line-height: 25px; }
  .row:not(.head):hover .cell { background: var(--bg-hover); }
  .cell.active { outline: 1px solid var(--accent); outline-offset: -1px; }
  .cell.null { color: var(--ink-3); font-style: italic; }
  .index { color: var(--ink-3); justify-content: flex-end; text-align: right; background: var(--bg-sunken); }
  .th { flex-direction: column; align-items: flex-start; justify-content: center; gap: 1px; cursor: pointer; position: relative; }
  .th:hover { background: var(--bg-hover); }
  .type { font-size: 10.5px; color: var(--ink-3); max-width: 100%; }
  .arrow { position: absolute; right: 8px; top: 9px; color: var(--accent); }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  .empty { padding: 16px; position: absolute; top: 34px; }
  .detail {
    width: 380px; flex: none; border-left: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0;
    background: var(--bg-raised);
  }
  .detail header { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: 1px solid var(--line); }
  .detail .type { font-size: 11px; }
  .note { margin: 8px 12px 0; font-size: 12px; color: var(--attention); }
  .detail p { margin: 8px 12px 0; }
  .value-full { flex: 1; overflow: auto; margin: 0; padding: 12px; font-size: 12px; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
