<script lang="ts">
  import type { DbCell, DbColumn, DbSort } from '../lib/types';
  import ResizeHandle from './ResizeHandle.svelte';

  let {
    columns,
    rows,
    sort = null,
    onsort,
    onnearend,
    empty = null,
  }: {
    columns: DbColumn[];
    rows: (DbCell | null)[][];
    sort?: DbSort | null;
    /** Absent : les en-têtes ne trient pas. */
    onsort?: (column: string) => void;
    /** Appelé en approchant du bas, pour charger la suite. */
    onnearend?: () => void;
    /** Texte affiché sans ligne ; `null` : rien. */
    empty?: string | null;
  } = $props();

  /** Hauteur fixe d'une ligne : la grille ne rend que les lignes visibles, calculées au défilement. */
  const ROW_H = 26;
  const OVERSCAN = 10;
  const COL_W = 200;

  /** Cellule ouverte dans le panneau de détail. */
  /** Largeur du détail choisie à la poignée ; `null` garde celle de la feuille de style. */
  let detailWidth = $state<number | null>(null);
  let bodyW = $state(0);

  let detail = $state<{ column: DbColumn; cell: DbCell | null } | null>(null);

  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewport = $state(600);

  // Lignes vidées : un nouveau contenu commence en haut, sans détail ouvert.
  $effect(() => {
    if (rows.length > 0) return;
    detail = null;
    if (scroller) scroller.scrollTop = 0;
    scrollTop = 0;
  });

  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN));
  const last = $derived(Math.min(rows.length, Math.ceil((scrollTop + viewport) / ROW_H) + OVERSCAN));
  const template = $derived(`56px repeat(${columns.length}, ${COL_W}px)`);

  function onscroll() {
    if (!scroller) return;
    scrollTop = scroller.scrollTop;
    viewport = scroller.clientHeight;
    // La suite est demandée un peu avant d'atteindre le bas.
    if (onnearend && scrollTop + viewport > rows.length * ROW_H - 20 * ROW_H) onnearend();
  }

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

<div class="body" bind:clientWidth={bodyW}>
  <div class="scroller" bind:this={scroller} {onscroll} bind:clientHeight={viewport}>
    <div class="row head" style:grid-template-columns={template}>
      <span class="cell index"></span>
      {#each columns as c, j (j)}
        <button class="cell th" class:sortable={!!onsort} onclick={() => onsort?.(c.name)} title={c.dataType}>
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
    {#if empty !== null && rows.length === 0}<p class="muted empty">{empty}</p>{/if}
  </div>

  {#if detail}
    {@const d = detail}
    <aside class="detail" style:--detail-w={detailWidth ? `${detailWidth}px` : null}>
      <ResizeHandle
        id="db.detail"
        edge="left"
        min={260}
        max={bodyW ? bodyW - 300 : Infinity}
        label="Largeur du détail de la valeur"
        bind:width={detailWidth}
      />
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

<style>
  .spacer { flex: 1; }
  .small { height: 24px; padding: 0 8px; font-size: 12px; }
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
  .th { flex-direction: column; align-items: flex-start; justify-content: center; gap: 1px; position: relative; }
  .th.sortable { cursor: pointer; }
  .th.sortable:hover { background: var(--bg-hover); }
  .type { font-size: 10.5px; color: var(--ink-3); max-width: 100%; }
  .arrow { position: absolute; right: 8px; top: 9px; color: var(--accent); }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  .empty { padding: 16px; position: absolute; top: 34px; }
  .detail {
    position: relative; width: var(--detail-w, 380px); flex: none; border-left: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0;
    background: var(--bg-raised);
  }
  .detail header { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: 1px solid var(--line); }
  .detail .type { font-size: 11px; }
  .note { margin: 8px 12px 0; font-size: 12px; color: var(--attention); }
  .detail p { margin: 8px 12px 0; }
  .value-full { flex: 1; overflow: auto; margin: 0; padding: 12px; font-size: 12px; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
