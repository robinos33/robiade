<script lang="ts">
  import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
  import { sql, MariaSQL, PostgreSQL } from '@codemirror/lang-sql';
  import { EditorState, Prec } from '@codemirror/state';
  import { drawSelection, EditorView, highlightActiveLine, keymap, lineNumbers, placeholder } from '@codemirror/view';
  import { api } from '../lib/api';
  import { robiadeTheme } from '../lib/codemirror';
  import type { DbConnection, DbConsoleHistory, DbConsoleReply, DbConsoleResult } from '../lib/types';
  import DbStatementConfirm from './DbStatementConfirm.svelte';
  import DbTable from './DbTable.svelte';
  import DbWriteCommit from './DbWriteCommit.svelte';

  let { connection, readOnly }: { connection: DbConnection; readOnly: boolean } = $props();

  /** Ce que l'éditeur vide montre : le langage de la console n'est pas le même d'un moteur à l'autre. */
  const EXAMPLE: Record<DbConnection['engine'], string> = {
    postgres: 'SELECT * FROM … — ⌘↵ exécute l’instruction sous le curseur',
    mariadb: 'SELECT * FROM … — ⌘↵ exécute l’instruction sous le curseur',
    mongo: '{"find": "collection", "filter": {"_id": {"$oid": "…"}}} — commande JSON, ⌘↵',
    elasticsearch: 'GET /index/_search puis le corps JSON à la ligne — ⌘↵',
  };

  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | undefined;

  let running = $state(false);
  let error = $state<string | null>(null);
  let result = $state<{ statement: string; result: DbConsoleResult } | null>(null);
  let confirming = $state<Extract<DbConsoleReply, { kind: 'confirm' }> | null>(null);
  /** Modification de prod exécutée dans sa transaction, en attente de Valider ou Annuler. */
  let pending = $state<(Extract<DbConsoleReply, { kind: 'pending' }> & { target: string }) | null>(null);
  let finishing = $state(false);
  /** Issue de la dernière modification de prod. */
  let notice = $state<string | null>(null);
  let past = $state<DbConsoleHistory[]>([]);
  let showHistory = $state(false);

  /** Le texte de l'éditeur survit à la fermeture de l'explorateur, pour ce poste seulement. */
  const draftKey = $derived(`robiade.console.${connection.id}`);
  function readDraft(key: string): string {
    try {
      return localStorage.getItem(key) ?? '';
    } catch {
      return '';
    }
  }
  function saveDraft(key: string, text: string) {
    try {
      localStorage.setItem(key, text);
    } catch {
      /* stockage indisponible : le brouillon n'est pas gardé */
    }
  }

  $effect(() => {
    if (!host) return;
    const key = draftKey;
    const editor = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: readDraft(key),
        extensions: [
          // ⌘↵ avant le clavier par défaut, qui l'emploie pour insérer une ligne.
          Prec.highest(keymap.of([{ key: 'Mod-Enter', run: () => (void run(), true) }])),
          lineNumbers(),
          highlightActiveLine(),
          drawSelection(),
          history(),
          placeholder(EXAMPLE[connection.engine]),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          // Elasticsearch et Mongo : requêtes REST ou commandes JSON, pas de SQL à colorer.
          connection.engine === 'elasticsearch' || connection.engine === 'mongo'
            ? []
            : sql({ dialect: connection.engine === 'mariadb' ? MariaSQL : PostgreSQL }),
          robiadeTheme,
          EditorView.updateListener.of((u) => {
            if (u.docChanged) saveDraft(key, u.state.doc.toString());
          }),
        ],
      }),
    });
    view = editor;
    editor.focus();
    return () => {
      editor.destroy();
      view = undefined;
    };
  });

  async function loadHistory() {
    try {
      past = await api.dbConsoleHistory(connection.id);
    } catch {
      past = [];
    }
  }
  $effect(() => {
    void connection.id;
    void loadHistory();
  });

  /** La sélection si elle existe, sinon l'instruction sous le curseur : c'est Rust qui la découpe. */
  async function run() {
    if (!view || running) return;
    const selection = view.state.selection.main;
    const text = selection.empty ? view.state.doc.toString() : view.state.sliceDoc(selection.from, selection.to);
    await send(text, selection.empty ? selection.head : null, null);
  }

  /** `confirmation` : réponse à la confirmation ; en prod, le nom de la connexion tapé. */
  async function send(text: string, cursor: number | null, confirmation: string | null, target = '') {
    running = true;
    error = null;
    notice = null;
    try {
      const reply = await api.runDbStatement(connection.id, text, cursor, confirmation);
      if (reply.kind === 'confirm') confirming = reply;
      else if (reply.kind === 'pending') pending = { ...reply, target };
      else result = { statement: reply.statement, result: reply.result };
    } catch (e) {
      error = String(e);
    } finally {
      running = false;
      void loadHistory();
    }
  }

  /** Seule l'instruction montrée dans la confirmation repart, telle quelle. */
  function confirmWrite(confirmation: string) {
    const { statement, target } = confirming!;
    confirming = null;
    void send(statement, null, confirmation, target);
  }

  async function finish(commit: boolean) {
    finishing = true;
    try {
      const done = await api.finishDbWrite(connection.id, commit);
      const n = done.affected;
      notice = done.committed
        ? `Validé (COMMIT) : ${n} ligne${n > 1 ? 's' : ''} écrite${n > 1 ? 's' : ''} en production.`
        : `Annulé (ROLLBACK) : rien n'a été écrit.`;
      if (done.sessionClosed) error = "La session n'était plus en lecture seule : elle est fermée, rouvre la connexion.";
      result = null;
    } catch (e) {
      error = String(e);
    } finally {
      pending = null;
      finishing = false;
    }
  }

  async function cancel() {
    try {
      await api.cancelDbStatement(connection.id);
    } catch (e) {
      error = String(e);
    }
  }

  function recall(entry: DbConsoleHistory) {
    showHistory = false;
    if (!view) return;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: entry.sql }, selection: { anchor: entry.sql.length } });
    view.focus();
  }

  const summary = $derived.by(() => {
    if (!result) return '';
    const r = result.result;
    const time = `${r.elapsedMs} ms`;
    if (r.affected !== null && r.columns.length === 0) return `${r.affected} ligne${r.affected > 1 ? 's' : ''} touchée${r.affected > 1 ? 's' : ''} · ${time}`;
    const n = r.rows.length;
    return `${n}${r.truncated ? '+' : ''} ligne${n > 1 ? 's' : ''}${r.truncated ? ' (tronqué à 1000)' : ''} · ${time}`;
  });
</script>

<div class="console">
  <div class="editor" bind:this={host}></div>

  <div class="bar">
    {#if running}
      <button class="btn small" onclick={() => void cancel()}>Annuler la requête</button>
      <span class="muted">exécution…</span>
    {:else}
      <button class="btn small primary" onclick={() => void run()} title="⌘↵">Exécuter ⌘↵</button>
    {/if}
    <span class="muted hint">la sélection, ou l'instruction sous le curseur</span>
    {#if connection.stage === 'prod'}
      <span class="tag prod">prod : lectures ; INSERT, UPDATE ou DELETE (avec WHERE) validés</span>
    {:else if readOnly}<span class="tag">lecture seule</span>{/if}
    <span class="spacer"></span>
    {#if result}<span class="muted mono">{summary}</span>{/if}
    <button class="btn small" onclick={() => (showHistory = !showHistory)} disabled={past.length === 0}>
      Historique {#if past.length}<span class="num">{past.length}</span>{/if}
    </button>
  </div>

  {#if showHistory}
    <ul class="history">
      {#each past as entry, i (i)}
        <li>
          <button onclick={() => recall(entry)} title={entry.sql}>
            <span class="mono ellipsis">{entry.sql}</span>
            <span class="muted when">{new Date(entry.at).toLocaleString()}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if error}<p class="error-text">{error}</p>{/if}

  <div class="result">
    {#if notice}
      <p class="done notice">{notice}</p>
    {:else if result}
      {#if result.result.columns.length}
        <DbTable columns={result.result.columns} rows={result.result.rows} empty="Aucune ligne." />
      {:else}
        <p class="muted done mono">{result.statement}<br />{summary}</p>
      {/if}
    {:else if !error}
      <p class="muted done">Écris une requête, puis <kbd>⌘↵</kbd>.</p>
    {/if}
  </div>
</div>

{#if confirming}
  {@const c = confirming}
  <DbStatementConfirm
    statement={c.statement}
    connection={c.connection}
    stage={connection.stage}
    prod={c.prod}
    target={c.target}
    onconfirm={confirmWrite}
    onclose={() => (confirming = null)}
  />
{/if}

{#if pending}
  {@const p = pending}
  <DbWriteCommit
    statement={p.statement}
    affected={p.affected}
    delayMs={p.delayMs}
    connection={connection.name}
    target={p.target}
    busy={finishing}
    onfinish={(commit) => void finish(commit)}
  />
{/if}

<style>
  .console { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; position: relative; }
  .editor { height: 38%; min-height: 120px; flex: none; border-bottom: 1px solid var(--line); overflow: hidden; }
  .bar { display: flex; align-items: center; gap: 8px; padding: 6px 12px; border-bottom: 1px solid var(--line); font-size: 12px; }
  .small { height: 24px; padding: 0 8px; font-size: 12px; }
  .hint { font-size: 11.5px; }
  .tag { font-size: 11px; padding: 0 6px; border-radius: 8px; border: 1px solid var(--line-strong); color: var(--ink-2); }
  .tag.prod { border-color: var(--danger); color: var(--danger); }
  .spacer { flex: 1; }
  .num { margin-left: 4px; color: var(--ink-3); }
  .history {
    position: absolute; right: 12px; top: calc(38% + 36px); z-index: 5; width: min(560px, 70%); max-height: 50%;
    overflow-y: auto; margin: 0; padding: 4px; list-style: none; background: var(--bg-raised);
    border: 1px solid var(--line-strong); border-radius: var(--radius-sm); box-shadow: var(--shadow);
  }
  .history button {
    display: flex; align-items: center; gap: 8px; width: 100%; padding: 5px 8px; border: 0; border-radius: var(--radius-sm);
    background: none; color: var(--ink); text-align: left; cursor: pointer; min-width: 0;
  }
  .history button:hover { background: var(--bg-hover); }
  .when { flex: none; font-size: 11px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; min-width: 0; }
  .error-text { color: var(--danger); margin: 8px 12px; font-size: 12.5px; overflow-wrap: anywhere; white-space: pre-wrap; }
  .result { flex: 1; display: flex; min-height: 0; min-width: 0; }
  .done { padding: 16px; margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  .notice { font-weight: 550; }
</style>
