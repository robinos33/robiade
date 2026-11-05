<script lang="ts">
  import { sidebar, toggleSidebar } from '../lib/sidebar.svelte';
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

  /** Ce que porte le bouton une fois la barre repliée. */
  const signal = $derived(app.signal);
  const toggleLabel = $derived(
    sidebar.collapsed ? `Afficher la barre latérale${signal ? ` · ${signal.text}` : ''}` : 'Replier la barre latérale',
  );

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

  type Section = 'projects' | 'worktrees';
  /** Le repli est retenu sur le poste ; la clé n'existe que pour une section repliée. */
  const foldKey = (section: Section) => `robiade.sidebar.${section}.collapsed`;
  function storedFold(section: Section): boolean {
    try {
      return localStorage.getItem(foldKey(section)) === '1';
    } catch {
      return false;
    }
  }
  const collapsed = $state<Record<Section, boolean>>({ projects: storedFold('projects'), worktrees: storedFold('worktrees') });
  function toggle(section: Section) {
    collapsed[section] = !collapsed[section];
    try {
      if (collapsed[section]) localStorage.setItem(foldKey(section), '1');
      else localStorage.removeItem(foldKey(section));
    } catch {
      // Stockage inaccessible : le repli vaut pour la session.
    }
  }

  /** Repliée, une section garde la ligne de la sélection, pour qu'on sache toujours où on est. */
  const showGlobal = $derived(!collapsed.projects || app.project === null);
  const shownProjects = $derived(collapsed.projects ? app.projects.filter((p) => p.root === app.project) : app.projects);
  const shownWorktrees = $derived(collapsed.worktrees ? app.worktrees.filter((w) => w.path === app.worktree) : app.worktrees);
  /** Agents de toutes les lignes d'une section : sa pastille, une fois repliée. */
  const liveInProjects = $derived(app.liveAgents.filter((a) => app.projects.some((p) => p.root === a.projectRoot)));
  const liveInWorktrees = $derived(app.liveAgents.filter((a) => app.worktrees.some((w) => w.path === a.worktreePath)));

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

<!-- Titre d'une section : un clic la replie ; repliée, elle montre son compte et sa pastille la plus forte. -->
{#snippet heading(section: Section, label: string, count: number, live: Agent[])}
  <h2 class="section-title">
    <button class="fold" aria-expanded={!collapsed[section]} onclick={() => toggle(section)}>
      <span class="chevron"><Icon name="next" size={12} /></span>
      <span class="label">{label}</span>
      {#if collapsed[section]}
        <span class="count">{count}</span>
        {@render dot(live)}
      {/if}
    </button>
  </h2>
{/snippet}

<svelte:window onclick={closeMenu} onkeydown={closeMenu} onblur={() => (menu = null)} />

<!-- Hors de la barre, pour rester à droite des feux tricolores quand elle est repliée. -->
<button
  class="toggle"
  onclick={toggleSidebar}
  aria-expanded={!sidebar.collapsed}
  aria-label={toggleLabel}
  title="{toggleLabel} (⌘B)"
>
  <Icon name="sidebar" />
  {#if sidebar.collapsed && signal}<span class="signal" class:hot={signal.hot}>{signal.count}</span>{/if}
</button>

<!-- Masquée et non démontée : la largeur réglée et l'état des composants survivent au repli. -->
<nav class:collapsed={sidebar.collapsed} style:--sidebar-w={width ? `${width}px` : null}>
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
    {#each NAV.filter((i) => i.view !== 'mr' || app.env?.forge) as item (item.view)}
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

  <div class="projects" class:collapsed={collapsed.projects}>
    {@render heading('projects', 'Projets', app.projects.length, liveInProjects)}
    {#if showGlobal}
      <button class="item" class:active={app.project === null} onclick={() => app.selectProject(null)}>
        <Icon name="globe" />
        <span class="label">Global</span>
      </button>
    {/if}
    <ul>
      {#each shownProjects as p (p.root)}
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
    <div class="projects worktrees" class:collapsed={collapsed.worktrees}>
      {@render heading('worktrees', 'Worktrees', app.worktrees.length, liveInWorktrees)}
      <ul>
        {#each shownWorktrees as w (w.path)}
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
  <!-- Collé en bas, même quand les sections repliées ne remplissent plus la hauteur. -->
  <div class="bottom">
    <UpdateStatus />
    <div class="foot">
      <button class="theme" onclick={cycleTheme} title={themeTitle} aria-label={themeTitle}>
        <Icon name={THEMES[theme.choice].icon} />
      </button>
      <button class="theme" class:active={app.view === 'settings'} onclick={() => (app.view = 'settings')} title="Réglages (⌘,)" aria-label="Réglages">
        <Icon name="settings" />
      </button>
    </div>
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
  nav.collapsed { display: none; }
  .titlebar { height: 30px; flex: none; }
  /* Même place barre ouverte ou repliée, juste à droite des feux : `--titlebar-inset` (app.css) la réserve. */
  .toggle {
    position: fixed; top: 4px; left: 72px; z-index: 30;
    display: grid; place-items: center; width: 24px; height: 22px; padding: 0;
    border: 0; border-radius: var(--radius-sm); background: none; color: var(--ink-3);
  }
  .toggle:hover { background: var(--bg-hover); color: var(--ink); }
  /* En coin, pour que le bouton garde sa largeur : `--titlebar-inset` couvre un compte à deux chiffres. */
  .signal {
    position: absolute; top: -3px; left: 15px; min-width: 15px; height: 15px; padding: 0 4px;
    border-radius: 8px; background: var(--info); box-shadow: 0 0 0 2px var(--bg);
    color: #fff; font-size: 10px; font-weight: 650; line-height: 15px; text-align: center;
    font-variant-numeric: tabular-nums;
  }
  .signal.hot { background: var(--accent); }
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
  .projects ul { overflow-y: auto; margin-top: 2px; }
  .worktrees { flex: 0 1 auto; max-height: 40%; margin-top: 12px; }
  /* Repliée, une section rend sa hauteur : Projets la prend d'office, Worktrees perd son plafond. */
  .projects.collapsed { flex: none; }
  .projects.collapsed + .worktrees:not(.collapsed) { flex: 1; max-height: none; }
  .fold {
    display: flex; align-items: center; gap: 6px; width: 100%; padding: 0 8px;
    border: 0; background: none; font: inherit; letter-spacing: inherit; text-transform: inherit; text-align: left;
  }
  .fold:hover { color: var(--ink-2); }
  .chevron { display: flex; transition: transform 120ms ease; }
  .fold[aria-expanded='true'] .chevron { transform: rotate(90deg); }
  .bottom { margin-top: auto; }
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
  .theme:hover, .theme.active { background: var(--bg-hover); color: var(--ink); }
  .dot.hot { background: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
</style>
