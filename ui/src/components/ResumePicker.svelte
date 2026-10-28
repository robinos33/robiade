<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { ago } from '../lib/format';
  import type { Agent, Session } from '../lib/types';
  import MrBadge from './MrBadge.svelte';

  /** Reprendre une session, comme `/resume` : filtrer, choisir, Entrée. */
  let query = $state('');
  let index = $state(0);
  let allProjects = $state(app.project === null);
  let contentIds = $state<string[]>([]);
  let input: HTMLInputElement | undefined = $state();
  let list: HTMLUListElement | undefined = $state();

  /** Onglet vivant qui porte déjà la session : on y va au lieu d'en lancer une deuxième. */
  const liveTab = (s: Session): Agent | undefined =>
    app.liveAgents.find((a) => (a.resumedFrom ?? a.id) === s.sessionId);

  const pool = $derived(app.conversations.filter((s) => allProjects || app.inProject(s.projectRoot)));

  const sessions = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return pool.slice(0, 200);
    const words = q.split(/\s+/);
    const byTitle = pool.filter((s) => {
      const text = `${s.title} ${s.projectName} ${s.worktreeName ?? ''} ${s.gitBranch ?? ''}`.toLowerCase();
      return words.every((w) => text.includes(w));
    });
    const seen = new Set(byTitle.map((s) => s.sessionId));
    const byContent = contentIds.flatMap((id) => {
      const s = pool.find((p) => p.sessionId === id);
      return s && !seen.has(id) ? [s] : [];
    });
    return [...byTitle, ...byContent].slice(0, 200);
  });

  $effect(() => input?.focus());

  // Recherche plein texte dans les conversations, après une pause de frappe.
  $effect(() => {
    const q = query.trim();
    contentIds = [];
    if (q.length < 3) return;
    const timer = setTimeout(() => {
      api.searchHistory(q).then(
        (hits) => query.trim() === q && (contentIds = hits.map((h) => h.sessionId)),
        () => {},
      );
    }, 250);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    void index;
    list?.querySelector('.active')?.scrollIntoView({ block: 'nearest' });
  });

  function close() {
    app.resumeOpen = false;
  }

  function pick(s: Session | undefined) {
    if (!s) return;
    close();
    const tab = liveTab(s);
    if (tab) app.openAgent(tab.id);
    else void app.spawn(s.projectPath, s.sessionId);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') index = Math.min(index + 1, sessions.length - 1);
    else if (e.key === 'ArrowUp') index = Math.max(index - 1, 0);
    else if (e.key === 'Enter') pick(sessions[index]);
    else if (e.key === 'Escape') close();
    else if (e.key === 'Tab' && app.project !== null) allProjects = !allProjects;
    else return;
    e.preventDefault();
  }
</script>

<div class="scrim" role="presentation" onclick={close}>
  <div class="dialog" role="dialog" aria-label="Reprendre une session" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
    <div class="head">
      <input
        placeholder="Reprendre une session : titre, projet, branche, ou un mot de la conversation…"
        bind:value={query}
        bind:this={input}
        {onkeydown}
        oninput={() => (index = 0)}
      />
      {#if app.project !== null}
        <button class="scope" onclick={() => (allProjects = !allProjects)} title="Tab pour basculer">
          {allProjects ? 'Tous les projets' : app.projects.find((p) => p.root === app.project)?.name}
        </button>
      {/if}
    </div>
    <ul bind:this={list}>
      {#each sessions as s, i (s.sessionId)}
        {@const tab = liveTab(s)}
        <li>
          <button class:active={i === index} onmouseenter={() => (index = i)} onclick={() => pick(s)}>
            <div class="line">
              <span class="title">{s.title}</span>
              {#if tab}<span class="open">onglet ouvert</span>{/if}
              {#if s.mergeRequestUrl}<MrBadge url={s.mergeRequestUrl} />{/if}
              <span class="when num">{ago(s.lastActivityAt, app.now)}</span>
            </div>
            <div class="line sub">
              <span>{s.projectName}</span>
              {#if s.worktreeName}<span class="mono">{s.worktreeName}</span>{/if}
              {#if s.gitBranch}<span class="mono">⎇ {s.gitBranch}</span>{/if}
              <span class="num">{s.userMessages} message{s.userMessages > 1 ? 's' : ''}</span>
            </div>
          </button>
        </li>
      {:else}
        <li class="none">{app.historyLoaded ? 'Aucune session ne correspond.' : 'Lecture de l’historique…'}</li>
      {/each}
    </ul>
    <footer><kbd>↑↓</kbd> choisir <kbd>⏎</kbd> reprendre <kbd>esc</kbd> fermer{#if app.project !== null} <kbd>tab</kbd> portée{/if}</footer>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 60; display: grid; justify-items: center; align-items: start; padding-top: 10vh; background: rgb(0 0 0 / 0.3); }
  .dialog { width: min(760px, 94vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow); overflow: hidden; outline: none; }
  .head { display: flex; align-items: center; border-bottom: 1px solid var(--line); }
  input { flex: 1; height: 50px; padding: 0 16px; border: 0; background: none; font-size: 15px; outline: none; }
  .scope { margin-right: 10px; height: 26px; padding: 0 10px; border: 1px solid var(--line-strong); border-radius: 13px; background: none; font-size: 12px; color: var(--ink-2); }
  ul { list-style: none; margin: 0; padding: 6px; max-height: 60vh; overflow-y: auto; }
  li > button { display: flex; flex-direction: column; gap: 3px; width: 100%; padding: 8px 10px; border: 0; border-radius: 6px; background: none; text-align: left; }
  li > button.active { background: var(--bg-active); }
  .line { display: flex; align-items: center; gap: 10px; min-width: 0; width: 100%; }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { font-size: 12px; color: var(--ink-3); }
  .sub span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .when { flex: none; font-size: 12px; color: var(--ink-3); }
  .open { flex: none; font-size: 11px; color: var(--ok); }
  .none { padding: 12px; color: var(--ink-3); }
  footer { display: flex; gap: 6px; align-items: center; padding: 8px 14px; border-top: 1px solid var(--line); font-size: 12px; color: var(--ink-3); }
</style>
