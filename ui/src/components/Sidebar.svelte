<script lang="ts">
  import { app, type View } from '../lib/state.svelte';
  import Icon from './Icon.svelte';

  const NAV: { view: View; label: string; icon: string; key: string }[] = [
    { view: 'inbox', label: 'Inbox', icon: 'inbox', key: '1' },
    { view: 'agents', label: 'Agents', icon: 'agents', key: '2' },
    { view: 'history', label: 'Historique', icon: 'history', key: '3' },
    { view: 'tasks', label: 'Tâches', icon: 'tasks', key: '4' },
    { view: 'stats', label: 'Statistiques', icon: 'stats', key: '5' },
  ];
  const LATER = [
    { label: 'Merge requests', icon: 'mr', milestone: 'v2' },
    { label: 'Environnements', icon: 'env', milestone: 'v3' },
    { label: 'Bases de données', icon: 'db', milestone: 'v4' },
  ];

  function badge(view: View): number | null {
    if (view === 'inbox') return app.attention.length || null;
    if (view === 'agents') return app.visibleAgents.filter((a) => a.status !== 'ended').length || null;
    if (view === 'tasks') return app.visibleTasks.filter((t) => t.status === 'doing').length || null;
    return null;
  }

  const liveIn = (root: string) => app.liveAgents.filter((a) => a.projectRoot === root);
</script>

<nav>
  <div class="titlebar" data-tauri-drag-region></div>

  <div class="brand" data-tauri-drag-region>
    <span class="logo" aria-hidden="true">R</span>
    <span class="name">Robiade</span>
    <button class="palette" onclick={() => (app.paletteOpen = true)} title="Palette de commandes">
      <kbd>⌘K</kbd>
    </button>
  </div>

  <button class="new" onclick={() => (app.newAgentOpen = true)}>
    <Icon name="plus" size={14} /> Nouvel agent <kbd>⌘N</kbd>
  </button>

  <ul class="nav">
    {#each NAV as item (item.view)}
      {@const n = badge(item.view)}
      <li>
        <button class="item" class:active={app.view === item.view} onclick={() => (app.view = item.view)}>
          <Icon name={item.icon} />
          <span class="label">{item.label}</span>
          {#if n}<span class="count" class:hot={item.view === 'inbox'}>{n}</span>{/if}
        </button>
      </li>
    {/each}
    {#each LATER as item (item.label)}
      <li>
        <button class="item later" disabled title="Prévu en {item.milestone}">
          <Icon name={item.icon} />
          <span class="label">{item.label}</span>
          <span class="milestone">{item.milestone}</span>
        </button>
      </li>
    {/each}
  </ul>

  <div class="projects">
    <h2 class="section-title">Projets</h2>
    <button class="item" class:active={app.project === null} onclick={() => (app.project = null)}>
      <Icon name="globe" />
      <span class="label">Global</span>
    </button>
    <ul>
      {#each app.projects as p (p.root)}
        {@const live = liveIn(p.root)}
        <li>
          <button class="item" class:active={app.project === p.root} onclick={() => (app.project = p.root)} title={p.root}>
            <Icon name="folder" />
            <span class="label">{p.name}</span>
            {#if live.some((a) => a.needsAttention)}
              <span class="dot hot" title="Un agent attend"></span>
            {:else if live.length}
              <span class="dot" title="{live.length} agent(s) actif(s)"></span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  </div>
</nav>

<style>
  nav {
    display: flex; flex-direction: column; min-height: 0;
    width: 232px; flex: none; padding: 0 10px 10px;
    background: var(--bg-raised); border-right: 1px solid var(--line);
  }
  .titlebar { height: 30px; flex: none; }
  .brand { display: flex; align-items: center; gap: 9px; padding: 4px 6px 14px; }
  .logo {
    display: grid; place-items: center; width: 22px; height: 22px; border-radius: 6px;
    background: var(--ink); color: var(--bg); font-weight: 800; font-size: 13px; position: relative;
  }
  .logo::after {
    content: ''; position: absolute; top: -2px; right: -2px; width: 7px; height: 7px;
    border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 2px var(--bg-raised);
  }
  .name { font-weight: 680; letter-spacing: -0.015em; font-size: 14px; flex: 1; }
  .palette { background: none; border: 0; padding: 0; }
  .new {
    display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 10px; margin-bottom: 12px;
    border-radius: var(--radius-sm); border: 1px solid var(--line-strong); background: var(--bg);
    font-weight: 550;
  }
  .new:hover { border-color: var(--accent); }
  .new kbd { margin-left: auto; }
  ul { list-style: none; margin: 0; padding: 0; }
  .item {
    display: flex; align-items: center; gap: 9px; width: 100%; height: 30px; padding: 0 8px;
    border: 0; border-radius: var(--radius-sm); background: none; color: var(--ink-2); text-align: left;
  }
  .item:hover:not(:disabled) { background: var(--bg-hover); color: var(--ink); }
  .item.active { background: var(--bg-active); color: var(--ink); }
  .item.later { opacity: 0.45; cursor: default; }
  .label { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { font-size: 11px; font-variant-numeric: tabular-nums; color: var(--ink-3); }
  .count.hot {
    color: #fff; background: var(--accent); border-radius: 9px; padding: 0 6px; font-weight: 650; line-height: 17px;
  }
  .milestone { font: 10px var(--mono); color: var(--ink-3); }
  .projects { margin-top: 18px; display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .projects h2 { padding: 0 8px; }
  .projects ul { overflow-y: auto; margin-top: 2px; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--info); }
  .dot.hot { background: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
</style>
