<script lang="ts">
  import { app, type View } from '../lib/state.svelte';
  import { cycleTheme, theme, type ThemeChoice } from '../lib/theme.svelte';
  import type { Agent, Worktree } from '../lib/types';
  import Icon from './Icon.svelte';
  import Logo from './Logo.svelte';
  import ResizeHandle from './ResizeHandle.svelte';
  import UpdateStatus from './UpdateStatus.svelte';
  import WorktreeRemoveDialog from './WorktreeRemoveDialog.svelte';

  /** Largeur choisie à la poignée ; `null` garde celle de la feuille de style. */
  let width = $state<number | null>(null);

  const NAV: { view: View; label: string; icon: string; key: string }[] = [
    { view: 'inbox', label: 'Inbox', icon: 'inbox', key: '1' },
    { view: 'agents', label: 'Agents', icon: 'agents', key: '2' },
    { view: 'history', label: 'Historique', icon: 'history', key: '3' },
    { view: 'mr', label: 'Merge requests', icon: 'mr', key: '6' },
    { view: 'tasks', label: 'Tâches', icon: 'tasks', key: '4' },
    { view: 'stats', label: 'Statistiques', icon: 'stats', key: '5' },
    { view: 'env', label: 'Environnements', icon: 'env', key: '7' },
    { view: 'code', label: 'Code', icon: 'code', key: '8' },
    { view: 'db', label: 'Bases de données', icon: 'db', key: '' },
  ];

  function badge(view: View): number | null {
    if (view === 'inbox') return app.attention.length || null;
    if (view === 'agents') return app.visibleAgents.filter((a) => a.status !== 'ended').length || null;
    if (view === 'mr') return app.pendingReviews.length + app.mrAttention.length || null;
    if (view === 'tasks') return app.dueTasks.length || app.visibleTasks.filter((t) => t.status === 'doing').length || null;
    return null;
  }

  /** Pastille : l'Inbox, et les tâches dès qu'une échéance tombe — leur compte remplace alors celui des « en cours ». */
  const hot = (view: View) => view === 'inbox' || (view === 'tasks' && app.dueTasks.length > 0);
  /** Échéances tombées hors du projet affiché : signalées partout, comptées seulement chez elles. */
  const dueElsewhere = $derived(app.allDueTasks.length - app.dueTasks.length);

  /** Commit de la build, au survol du nom : c'est lui qui dit si une mise à jour a été prise. */
  const build = $derived.by(() => {
    const env = app.env;
    if (!env?.commit) return 'Build hors install.sh : pas de mise à jour automatique';
    return `Build ${env.commit.slice(0, 7)}${env.dirty ? ', avec des modifications non commitées' : ''}`;
  });

  const THEMES: Record<ThemeChoice, { label: string; icon: string; next: string }> = {
    system: { label: 'Système', icon: 'theme-system', next: 'Clair' },
    light: { label: 'Clair', icon: 'sun', next: 'Sombre' },
    dark: { label: 'Sombre', icon: 'moon', next: 'Système' },
  };
  const themeTitle = $derived(`Thème : ${THEMES[theme.choice].label} (clic : ${THEMES[theme.choice].next})`);

  const liveIn = (root: string) => app.liveAgents.filter((a) => a.projectRoot === root);
  const liveInWorktree = (path: string) => app.liveAgents.filter((a) => a.worktreePath === path);

  /** Menu contextuel d'un worktree, ouvert au clic droit à l'endroit du pointeur. */
  let menu = $state<{ worktree: Worktree; x: number; y: number } | null>(null);
  let removing = $state<Worktree | null>(null);

  function openMenu(e: MouseEvent, worktree: Worktree) {
    e.preventDefault();
    // Les worktrees sont en bas de la barre : le menu remonte plutôt que de sortir de la fenêtre.
    menu = { worktree, x: e.clientX, y: Math.min(e.clientY, window.innerHeight - 48) };
  }

  function closeMenu(e: KeyboardEvent | MouseEvent) {
    if (!menu || (e instanceof KeyboardEvent && e.key !== 'Escape')) return;
    menu = null;
    if (e instanceof KeyboardEvent) e.preventDefault();
  }
</script>

<!-- Pastille d'un ensemble d'agents vivants : l'accent si l'un attend, le bleu s'il en tourne. -->
{#snippet dot(live: Agent[])}
  {#if live.some((a) => a.needsAttention)}
    <span class="dot hot" title="Un agent attend"></span>
  {:else if live.length}
    <span class="dot" title="{live.length} agent(s) actif(s)"></span>
  {/if}
{/snippet}

<svelte:window onclick={closeMenu} onkeydown={closeMenu} onblur={() => (menu = null)} />

<nav style:--sidebar-w={width ? `${width}px` : null}>
  <ResizeHandle id="sidebar" edge="right" min={180} max={360} label="Largeur de la barre latérale" bind:width />
  <div class="titlebar" data-tauri-drag-region></div>

  <div class="brand" data-tauri-drag-region>
    <Logo />
    <span class="name" title={build}>Robiade</span>
    <button class="palette" onclick={() => (app.paletteOpen = true)} title="Palette de commandes">
      <kbd>⌘K</kbd>
    </button>
  </div>

  <button class="new" onclick={() => app.newAgent()} title={app.project === null ? 'Choisir un projet' : '⇧⌘N pour choisir un autre projet'}>
    <Icon name="plus" size={14} /> Nouvel agent <kbd>⌘N</kbd>
  </button>

  <ul class="nav">
    {#each NAV as item (item.view)}
      {@const n = badge(item.view)}
      <li>
        <button class="item" class:active={app.view === item.view} onclick={() => (app.view = item.view)}>
          <Icon name={item.icon} />
          <span class="label">{item.label}</span>
          {#if item.view === 'tasks' && app.dueTasks.length === 0 && dueElsewhere > 0}
            <span class="dot hot" title="{dueElsewhere} échéance(s) tombée(s) hors de ce projet"></span>
          {:else if n}
            <span class="count" class:hot={hot(item.view)} title={item.view === 'tasks' && hot('tasks') ? 'Échéance aujourd’hui ou dépassée' : undefined}>{n}</span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>

  <div class="projects">
    <h2 class="section-title">Projets</h2>
    <button class="item" class:active={app.project === null} onclick={() => app.selectProject(null)}>
      <Icon name="globe" />
      <span class="label">Global</span>
    </button>
    <ul>
      {#each app.projects as p (p.root)}
        <li>
          <button class="item" class:active={app.project === p.root && app.worktree === null} onclick={() => app.selectProject(p.root)} title={p.root}>
            <Icon name="folder" />
            <span class="label">{p.name}</span>
            {@render dot(liveIn(p.root))}
          </button>
        </li>
      {/each}
    </ul>
  </div>

  {#if app.worktrees.length > 0}
    <div class="projects worktrees">
      <h2 class="section-title">Worktrees</h2>
      <ul>
        {#each app.worktrees as w (w.path)}
          <li>
            <button
              class="item"
              class:active={app.worktree === w.path}
              onclick={() => app.selectWorktree(w)}
              oncontextmenu={(e) => openMenu(e, w)}
              title={w.path}
            >
              <Icon name="branch" />
              <span class="label">{w.name}</span>
              {#if app.project === null}<span class="of">{w.projectName}</span>{/if}
              {@render dot(liveInWorktree(w.path))}
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
  <UpdateStatus />
  <div class="foot">
    <button class="theme" onclick={cycleTheme} title={themeTitle} aria-label={themeTitle}>
      <Icon name={THEMES[theme.choice].icon} />
    </button>
  </div>
</nav>

{#if menu}
  {@const target = menu.worktree}
  <div class="menu" role="menu" style:left="{menu.x}px" style:top="{menu.y}px">
    <button class="danger" role="menuitem" onclick={() => ((removing = target), (menu = null))}>
      Supprimer le worktree…
    </button>
  </div>
{/if}
{#if removing}<WorktreeRemoveDialog worktree={removing} onclose={() => (removing = null)} />{/if}

<style>
  nav {
    position: relative; display: flex; flex-direction: column; min-height: 0;
    width: var(--sidebar-w, 232px); flex: none; padding: 0 10px 10px;
    background: var(--bg-raised); border-right: 1px solid var(--line);
  }
  .titlebar { height: 30px; flex: none; }
  .brand { display: flex; align-items: center; gap: 9px; padding: 4px 6px 14px; }
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
  .label { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { font-size: 11px; font-variant-numeric: tabular-nums; color: var(--ink-3); }
  .count.hot {
    color: #fff; background: var(--accent); border-radius: 9px; padding: 0 6px; font-weight: 650; line-height: 17px;
  }
  .projects { margin-top: 18px; display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .projects h2 { padding: 0 8px; }
  .projects ul { overflow-y: auto; margin-top: 2px; }
  .worktrees { flex: 0 1 auto; max-height: 40%; margin-top: 12px; }
  .of { font-size: 11px; color: var(--ink-3); max-width: 70px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--info); }
  .menu {
    position: fixed; z-index: 90; min-width: 180px; padding: 4px; border-radius: 8px;
    background: var(--bg-raised); border: 1px solid var(--line-strong); box-shadow: var(--shadow);
  }
  .menu button {
    display: block; width: 100%; padding: 6px 10px; border: 0; border-radius: var(--radius-sm);
    background: none; text-align: left; color: var(--ink);
  }
  .menu button:hover { background: var(--bg-hover); }
  .menu .danger { color: var(--danger); }
  .foot { display: flex; margin-top: 8px; }
  .theme {
    display: grid; place-items: center; width: 28px; height: 28px; padding: 0;
    border: 0; border-radius: var(--radius-sm); background: none; color: var(--ink-3);
  }
  .theme:hover { background: var(--bg-hover); color: var(--ink); }
  .dot.hot { background: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
</style>
