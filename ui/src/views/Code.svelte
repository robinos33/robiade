<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { api } from '../lib/api';
  import { ancestors, baseName, buildTree, parentOf, visibleRows } from '../lib/fileTree';
  import { count, fileSize } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import type { ProjectFile } from '../lib/types';
  import FileFinder from '../components/FileFinder.svelte';
  import FilePreview, { copySource } from '../components/FilePreview.svelte';
  import Icon from '../components/Icon.svelte';

  const root = $derived(app.codeRoot);
  const rootName = $derived.by(() => {
    if (app.worktree) return app.worktrees.find((w) => w.path === app.worktree)?.name ?? baseName(app.worktree);
    return app.projects.find((p) => p.root === app.project)?.name ?? baseName(app.project ?? '');
  });

  let files = $state<string[] | null>(null);
  let secrets = $state(new Set<string>());
  let listError = $state<string | null>(null);
  const tree = $derived(buildTree(files ?? []));
  const expanded = new SvelteSet<string>();
  const rows = $derived(visibleRows(tree, expanded));

  /** Ligne sous le curseur clavier : un dossier ou un fichier. */
  let cursor = $state<string | null>(null);
  /** Fichier affiché à droite, et ce qu'en a renvoyé Rust. */
  let opened = $state<string | null>(null);
  let file = $state<ProjectFile | null>(null);
  let fileError = $state<string | null>(null);
  let source = $state(false);
  /** Derniers fichiers ouverts dans ce projet, le plus récent en tête : ce que ⌘P propose d'abord. */
  let recent = $state<string[]>([]);
  let list: HTMLUListElement | undefined = $state();

  async function loadFiles(current: string) {
    listError = null;
    try {
      const found = await api.projectFiles(current);
      if (root !== current) return;
      files = found.files;
      secrets = new Set(found.secrets);
    } catch (e) {
      if (root === current) listError = String(e);
    }
  }

  // Un autre projet : tout repart de zéro. Seule la racine est suivie, pas ce que l'effet remet à zéro.
  $effect(() => {
    const current = root;
    untrack(() => {
      files = null;
      expanded.clear();
      cursor = opened = null;
      recent = [];
      file = null;
      fileError = null;
      if (current) void loadFiles(current);
    });
  });

  function open(path: string, reveal = false) {
    const current = root;
    if (!current) return;
    cursor = opened = path;
    recent = [path, ...recent.filter((p) => p !== path)].slice(0, 10);
    file = null;
    fileError = null;
    source = false;
    api.readProjectFile(current, path, reveal).then(
      (f) => root === current && opened === path && (file = f),
      (e) => root === current && opened === path && (fileError = String(e)),
    );
  }

  function toggle(dir: string) {
    cursor = dir;
    if (expanded.has(dir)) expanded.delete(dir);
    else expanded.add(dir);
  }

  /** Ouvre `path` depuis l'extérieur de l'arbre : ses dossiers se déplient, la ligne vient à l'écran. */
  async function reveal(path: string) {
    for (const dir of ancestors(path)) expanded.add(dir);
    open(path);
    await tick();
    list?.querySelector(`[data-path="${CSS.escape(path)}"]`)?.scrollIntoView({ block: 'center' });
  }

  async function moveTo(path: string) {
    cursor = path;
    if (!tree.has(path)) open(path);
    await tick();
    list?.querySelector(`[data-path="${CSS.escape(path)}"]`)?.scrollIntoView({ block: 'nearest' });
  }

  /** ↑ ↓ parcourent, → déplie ou descend, ← replie ou remonte, ↵ déplie un dossier. */
  function onkeydown(e: KeyboardEvent) {
    if (e.metaKey || e.ctrlKey || e.altKey || rows.length === 0) return;
    const at = rows.findIndex((r) => r.path === cursor);
    const row = rows[at];
    if (e.key === 'ArrowDown') void moveTo(rows[Math.min(at + 1, rows.length - 1)]!.path);
    else if (e.key === 'ArrowUp') void moveTo(rows[Math.max(at - 1, 0)]!.path);
    else if (e.key === 'ArrowRight' && row?.dir) {
      if (!expanded.has(row.path)) expanded.add(row.path);
      else if (rows[at + 1]?.depth === row.depth + 1) void moveTo(rows[at + 1]!.path);
    } else if (e.key === 'ArrowLeft' && row) {
      if (row.dir && expanded.has(row.path)) expanded.delete(row.path);
      else if (parentOf(row.path)) void moveTo(parentOf(row.path));
    } else if (e.key === 'Enter' && row?.dir) toggle(row.path);
    else return;
    e.preventDefault();
  }

  const content = $derived(file?.state === 'content' ? file : null);
</script>

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Code</h1>
    {#if root}
      <span class="muted" title={root}>{rootName}</span>
      {#if files}<span class="muted num">{count(files.length)} fichiers</span>{/if}
    {/if}
    <span class="spacer"></span>
    {#if root}
      <button class="btn ghost" onclick={() => (app.fileFinderOpen = true)} disabled={!files}>
        <Icon name="search" size={14} /> Aller au fichier <kbd>⌘P</kbd>
      </button>
      <button class="btn ghost" onclick={() => root && void loadFiles(root)} title="Relire la liste des fichiers">Recharger</button>
    {/if}
  </header>

  {#if !root}
    <div class="empty">
      <div>
        <strong>Choisis un projet.</strong>L'onglet Code parcourt, en lecture seule, le projet ou le worktree
        sélectionné dans la barre latérale.
      </div>
    </div>
  {:else}
    <div class="split">
      <ul class="tree" role="tree" bind:this={list} tabindex="0" {onkeydown} aria-label="Fichiers de {rootName}">
        {#if listError}
          <li class="empty" role="none">{listError}</li>
        {:else if !files}
          <li class="empty" role="none">Lecture des fichiers…</li>
        {:else if files.length === 0}
          <li class="empty" role="none">Aucun fichier suivi ni à suivre.</li>
        {/if}
        {#each rows as row (row.path)}
          <li role="none">
            <button
              class="row"
              role="treeitem"
              aria-selected={row.path === opened}
              aria-expanded={row.dir ? expanded.has(row.path) : undefined}
              class:cursor={row.path === cursor}
              class:opened={row.path === opened}
              style:padding-left="{8 + row.depth * 14}px"
              data-path={row.path}
              tabindex="-1"
              onclick={() => (row.dir ? toggle(row.path) : open(row.path))}
              title={row.path}
            >
              {#if row.dir}
                <span class="chevron" class:open={expanded.has(row.path)}><Icon name="next" size={12} /></span>
                <Icon name="folder" size={14} />
              {:else}
                <span class="chevron"></span>
                <Icon name={secrets.has(row.path) ? 'lock' : 'file'} size={14} />
              {/if}
              <span class="name">{row.name}</span>
            </button>
          </li>
        {/each}
      </ul>

      <div class="detail">
        {#if opened}
          <header class="detail-head">
            <span class="mono path selectable">{opened}</span>
            {#if content && (content.kind === 'html' || content.kind === 'markdown')}
              <button class="btn ghost" onclick={() => (source = !source)}>{source ? 'Rendu' : 'Source'}</button>
            {/if}
            {#if content?.text}
              {@const text = content.text}
              <button class="btn ghost" onclick={() => void copySource(text)}>Copier la source</button>
            {/if}
          </header>
          {#if file?.state === 'secret'}
            <div class="empty">
              <div>
                <strong>Fichier de secrets : non chargé.</strong>Son contenu s'afficherait en clair, y compris dans
                un partage d'écran.
              </div>
              <button class="btn" onclick={() => opened && open(opened, true)}>Afficher quand même</button>
            </div>
          {:else if file?.state === 'tooLarge'}
            <div class="empty">
              <div><strong>Trop lourd pour l'aperçu.</strong>{fileSize(file.size)}, pour une limite de {fileSize(file.limit)}.</div>
            </div>
          {:else if file?.state === 'binary'}
            <div class="empty">
              <div><strong>Fichier binaire.</strong>{fileSize(file.size)}, sans aperçu.</div>
            </div>
          {:else}
            <FilePreview name={baseName(opened)} {content} error={fileError} {source} />
          {/if}
        {:else}
          <div class="empty">
            <div><strong>Choisis un fichier.</strong>Ou cherche-le par son nom avec <kbd>⌘P</kbd>. Lecture seule : rien ne s'écrit depuis Robiade.</div>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</section>

{#if app.fileFinderOpen && files}
  <FileFinder {files} {secrets} {recent} onpick={(path) => void reveal(path)} onclose={() => (app.fileFinderOpen = false)} />
{/if}

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .split { display: grid; grid-template-columns: minmax(240px, 26%) 1fr; min-height: 0; flex: 1; }
  .tree {
    list-style: none; margin: 0; padding: 6px; overflow: auto; border-right: 1px solid var(--line);
    outline: none;
  }
  .row {
    display: flex; align-items: center; gap: 5px; width: 100%; height: 24px; padding-right: 8px;
    border: 0; border-radius: var(--radius-sm); background: none; color: var(--ink-2); text-align: left;
  }
  .row:hover { background: var(--bg-hover); color: var(--ink); }
  .row.opened { background: var(--bg-active); color: var(--ink); }
  .tree:focus-within .row.cursor { box-shadow: inset 0 0 0 1px var(--accent); }
  .chevron { display: inline-grid; place-items: center; width: 12px; flex: none; color: var(--ink-3); }
  .chevron.open { transform: rotate(90deg); }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .detail { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .detail-head {
    display: flex; align-items: center; gap: 8px; height: 40px; padding: 0 12px 0 18px; flex: none;
    border-bottom: 1px solid var(--line);
  }
  .path { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .detail > .empty { flex: 1; align-content: center; }
</style>
