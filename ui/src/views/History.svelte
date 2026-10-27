<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { ago, count, dateTime, time, tokens } from '../lib/format';
  import type { Entry, SearchHit } from '../lib/types';
  import MrBadge from '../components/MrBadge.svelte';
  import Icon from '../components/Icon.svelte';

  const PAGE = 200;
  let query = $state('');
  let limit = $state(PAGE);
  let entries = $state<Entry[] | null>(null);
  let loadError = $state<string | null>(null);

  /** Résultats plein texte de la dernière recherche, par ordre de pertinence. */
  let hits = $state<SearchHit[]>([]);
  const snippets = $derived(new Map(hits.map((h) => [h.sessionId, h.snippet])));

  $effect(() => {
    const q = query.trim();
    if (q.length < 2) {
      hits = [];
      return;
    }
    const timer = setTimeout(() => {
      api.searchHistory(q).then(
        (found) => query.trim() === q && (hits = found),
        (e) => app.notify(`Recherche impossible : ${e}`),
      );
    }, 150);
    return () => clearTimeout(timer);
  });

  // Titre, projet ou branche d'abord (du plus récent au plus ancien), puis le contenu des échanges.
  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return app.visibleHistory;
    const byMeta = app.visibleHistory.filter((s) =>
      `${s.title} ${s.projectName} ${s.gitBranch ?? ''}`.toLowerCase().includes(q),
    );
    const listed = new Set(byMeta.map((s) => s.sessionId));
    const visible = new Map(app.visibleHistory.map((s) => [s.sessionId, s]));
    const byContent = hits.flatMap((h) => {
      const s = visible.get(h.sessionId);
      return s && !listed.has(s.sessionId) ? [s] : [];
    });
    return [...byMeta, ...byContent];
  });
  const selected = $derived(app.history.find((s) => s.sessionId === app.selectedSessionId));

  $effect(() => {
    const id = app.selectedSessionId;
    entries = null;
    loadError = null;
    if (!id) return;
    api.readTranscript(id).then(
      (e) => app.selectedSessionId === id && (entries = e),
      (e) => (loadError = String(e)),
    );
  });
</script>

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Historique</h1>
    <span class="muted num">{count(filtered.length)} sessions</span>
    <span class="spacer"></span>
    <div class="search">
      <Icon name="search" size={14} />
      <input class="field" placeholder="Titre, projet, branche, contenu…" bind:value={query} oninput={() => (limit = PAGE)} />
    </div>
  </header>

  <div class="split">
    <ul class="list">
      {#each filtered.slice(0, limit) as s (s.sessionId)}
        <li>
          <button class="session" class:active={s.sessionId === app.selectedSessionId} onclick={() => (app.selectedSessionId = s.sessionId)}>
            <span class="title">{s.title}</span>
            {#if snippets.get(s.sessionId)}
              <span class="snippet">
                {#each snippets.get(s.sessionId) ?? [] as [text, hit], i (i)}{#if hit}<mark>{text}</mark>{:else}{text}{/if}{/each}
              </span>
            {/if}
            <span class="meta">
              <span class="proj">{s.projectName}</span>
              {#if s.gitBranch && s.gitBranch !== 'HEAD'}<span class="mono ellipsis">{s.gitBranch}</span>{/if}
              <span class="spacer"></span>
              {#if s.mergeRequestUrl}<MrBadge url={s.mergeRequestUrl} />{/if}
              <span class="num">{ago(s.lastActivityAt, app.now)}</span>
            </span>
          </button>
        </li>
      {:else}
        <li class="empty">{app.historyLoaded ? 'Aucune session ne correspond.' : 'Lecture de l’historique…'}</li>
      {/each}
      {#if filtered.length > limit}
        <li><button class="btn more" onclick={() => (limit += PAGE)}>Afficher {Math.min(PAGE, filtered.length - limit)} de plus</button></li>
      {/if}
    </ul>

    <div class="detail">
      {#if selected}
        <div class="detail-head">
          <div class="readonly"><Icon name="lock" size={12} /> Lecture seule</div>
          <h2 class="selectable">{selected.title}</h2>
          <div class="facts">
            <span>{selected.projectName}</span>
            {#if selected.gitBranch}<span class="mono">{selected.gitBranch}</span>{/if}
            <span>{dateTime(selected.startedAt)}</span>
            <span class="num">{tokens(selected.tokens.total)} tokens</span>
            <span class="num">{selected.toolCalls} outils</span>
            {#if selected.mergeRequestUrl}<MrBadge url={selected.mergeRequestUrl} />{/if}
            <span class="spacer"></span>
            <button class="btn" onclick={() => app.spawn(selected.projectPath, selected.sessionId)}>
              <Icon name="resume" size={12} /> Reprendre dans Robiade
            </button>
          </div>
        </div>
        <ol class="transcript selectable">
          {#if loadError}
            <li class="muted">{loadError}</li>
          {:else if entries === null}
            <li class="muted">Chargement…</li>
          {:else}
            {#each entries as e, i (i)}
              <li class="entry {e.kind}">
                {#if e.kind === 'tool'}
                  <span class="mono">⏵ {e.text}</span>
                {:else}
                  <div class="who">{e.kind === 'user' ? 'Toi' : 'Claude'} <span class="muted">{time(e.at)}</span></div>
                  <div class="text">{e.text}</div>
                {/if}
              </li>
            {/each}
          {/if}
        </ol>
      {:else}
        <div class="empty">
          <div><strong>Choisis une session.</strong>Toutes tes sessions Claude Code, y compris celles d'avant Robiade, en lecture seule.</div>
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .search { position: relative; width: 300px; color: var(--ink-3); }
  .search :global(svg) { position: absolute; left: 9px; top: 8px; }
  .search .field { padding-left: 30px; }
  .split { display: grid; grid-template-columns: minmax(340px, 38%) 1fr; min-height: 0; flex: 1; }
  ul.list { list-style: none; margin: 0; padding: 6px; overflow-y: auto; border-right: 1px solid var(--line); }
  .session {
    display: flex; flex-direction: column; gap: 3px; width: 100%; padding: 8px 10px; text-align: left;
    border: 0; border-radius: var(--radius-sm); background: none;
  }
  .session:hover { background: var(--bg-hover); }
  .session.active { background: var(--bg-active); }
  .title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink); }
  .snippet {
    font-size: 12px; color: var(--ink-2); line-height: 1.4;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
  }
  mark { background: var(--accent-soft); color: var(--ink); border-radius: 2px; padding: 0 1px; }
  .meta { display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--ink-3); min-width: 0; }
  .proj { color: var(--ink-2); }
  .spacer { flex: 1; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 160px; }
  .more { margin: 8px auto; display: flex; }
  .detail { display: flex; flex-direction: column; min-height: 0; }
  .detail-head { padding: 16px 24px 14px; border-bottom: 1px solid var(--line); }
  .readonly {
    display: inline-flex; align-items: center; gap: 5px; font-size: 11px; color: var(--ink-3);
    border: 1px solid var(--line-strong); border-radius: 10px; padding: 1px 8px; margin-bottom: 8px;
  }
  h2 { margin: 0 0 8px; font-size: 16px; font-weight: 620; letter-spacing: -0.01em; }
  .facts { display: flex; align-items: center; flex-wrap: wrap; gap: 6px 14px; color: var(--ink-2); font-size: 12px; }
  .transcript { list-style: none; margin: 0; padding: 16px 24px 48px; overflow-y: auto; display: flex; flex-direction: column; gap: 14px; }
  .entry.tool { color: var(--ink-3); font-size: 12px; margin: -8px 0; padding-left: 14px; }
  .entry.tool span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; display: block; }
  .who { font-size: 12px; font-weight: 600; margin-bottom: 4px; }
  .entry.user .who { color: var(--accent); }
  .text { white-space: pre-wrap; word-break: break-word; line-height: 1.55; max-width: 78ch; }
  .entry.user .text { background: var(--bg-raised); border: 1px solid var(--line); border-radius: var(--radius); padding: 10px 12px; }
</style>
