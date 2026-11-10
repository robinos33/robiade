<script lang="ts">
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { attach, dropPaths, refit, resizeFont, terminalAt, terminalShortcut } from '../lib/terminals';
  import { panelDrag } from '../lib/panelDrag.svelte';
  import { terminalFull } from '../lib/terminalFull.svelte';
  import { linkedPath, readLinkedFile, viewable } from '../lib/links';
  import { baseName } from '../lib/fileTree';
  import { ago, dollars, duration, shortPath, tokens } from '../lib/format';
  import StatusBadge from '../components/StatusBadge.svelte';
  import MrBadge from '../components/MrBadge.svelte';
  import ModeBadge from '../components/ModeBadge.svelte';
  import GaugeBar from '../components/GaugeBar.svelte';
  import AssetViewer from '../components/AssetViewer.svelte';
  import ChangesPanel from '../components/ChangesPanel.svelte';
  import LifecycleBar from '../components/LifecycleBar.svelte';
  import ShortcutBar from '../components/ShortcutBar.svelte';
  import LaunchPicker from '../components/LaunchPicker.svelte';
  import Icon from '../components/Icon.svelte';
  import ResizeHandle from '../components/ResizeHandle.svelte';
  import TerminalSearch from '../components/TerminalSearch.svelte';
  import type { Agent, Lifecycle, Session } from '../lib/types';

  /** Sessions reprenables proposées sous les onglets, comme le fait `/resume`. */
  const RECENT_SESSIONS = 8;
  /** Largeur gardée au terminal quand on élargit un panneau : une soixantaine de colonnes. */
  const TERMINAL_MIN = 480;

  /** Largeurs choisies aux poignées ; `null` garde celles de la feuille de style. */
  let railWidth = $state<number | null>(null);
  let sideWidth = $state<number | null>(null);
  const SIDE_MIN = 220;
  /**
   * Largeurs rendues, pour borner les panneaux. La liste passe d'abord, en supposant le panneau de
   * droite à son minimum ; celui-ci prend ce qu'elle laisse. Des bornes qui dépendraient l'une de
   * l'autre oscilleraient d'un rendu à l'autre.
   */
  let splitW = $state(0);
  let railW = $state(0);

  let container: HTMLDivElement | undefined = $state();
  /** Chemin de l'asset ouvert en aperçu, pour l'agent actif. */
  let viewing = $state<string | null>(null);
  /** Lien `file://` ouvert par ⌘+clic dans le terminal d'un agent. */
  let linked = $state<{ agentId: string; uri: string; line?: number } | null>(null);
  /** Barre de recherche (⌘F) ouverte sur le terminal de l'agent actif. */
  let finding = $state(false);
  let search: TerminalSearch | undefined = $state();
  /** Des fichiers du Finder survolent le terminal. */
  let dropping = $state(false);
  /** Onglet du panneau de droite. */
  let side = $state<'session' | 'diff'>('session');

  const active = $derived(app.agents.find((a) => a.id === app.activeAgentId) ?? app.visibleAgents[0]);
  const session = $derived(active ? app.sessionOf(active) : undefined);
  /** Frise du ticket de l'agent actif, lue par `LifecycleBar`. */
  let lifecycle = $state<Lifecycle | null>(null);
  // La MR de la frise est lue par branche, quel que soit son état et sur les deux forges ; à défaut,
  // le lien de la session.
  const mr = $derived.by(() => {
    const found = lifecycle?.mergeRequest;
    if (found) return { url: found.webUrl, state: found.state, title: found.title };
    return active ? app.mergeRequestOf(active) : null;
  });
  const sessionId = $derived(active ? active.sessionId : '');
  const viewed = $derived(active?.assets.find((a) => a.path === viewing));

  const running = $derived(app.visibleAgents.filter((a) => a.status !== 'ended'));
  /** Onglets terminés et sessions de l'historique (y compris celles d'iTerm) sans onglet, les plus récents d'abord. */
  type Recent = { kind: 'tab'; agent: Agent; at: number } | { kind: 'session'; session: Session; at: number };
  const recent = $derived.by((): Recent[] => {
    const tabs = app.visibleAgents.filter((a) => a.status === 'ended');
    const covered = new Set(app.agents.map((a) => a.sessionId));
    const sessions = app.visibleHistory
      .filter((s) => !covered.has(s.sessionId))
      .sort((a, b) => b.lastActivityAt - a.lastActivityAt)
      .slice(0, RECENT_SESSIONS);
    return [
      ...tabs.map((agent): Recent => ({ kind: 'tab', agent, at: agent.lastActivityAt })),
      ...sessions.map((session): Recent => ({ kind: 'session', session, at: session.lastActivityAt })),
    ].sort((a, b) => b.at - a.at);
  });

  /** Nom en cours d'édition, et l'agent qu'il concerne ; `null` hors édition. */
  let draft = $state<{ id: string; value: string } | null>(null);
  let nameField: HTMLInputElement | undefined = $state();
  const renaming = $derived(draft !== null && draft.id === active?.id);

  $effect(() => nameField?.select());

  function startRename(id: string) {
    app.openAgent(id);
    draft = { id, value: app.agents.find((a) => a.id === id)?.name ?? '' };
  }

  async function saveName() {
    if (draft === null) return;
    const { id, value } = draft;
    draft = null;
    await api.renameAgent(id, value).catch((e) => app.notify(String(e)));
    void app.refreshHistory();
  }

  function onNameKey(e: KeyboardEvent) {
    if (e.key === 'Enter') void saveName();
    if (e.key === 'Escape') draft = null;
  }

  async function forget(id: string) {
    await api.forgetAgent(id).catch((e) => app.notify(String(e)));
  }

  // Sur l'identifiant seul : chaque `agents:changed` remplace l'objet, et rattacher le terminal
  // lui redonnerait le focus, ce qui refermerait le champ de renommage.
  const activeId = $derived(active?.id);
  $effect(() => {
    const id = activeId;
    if (!id || !container) return;
    attach(id, container, (uri, line) => (linked = { agentId: id, uri, line }));
    const observer = new ResizeObserver(() => { if (!panelDrag.active) refit(id); });
    observer.observe(container);
    return () => observer.disconnect();
  });

  // Pendant le glisser d'une poignée, un seul `fit`, au relâchement : un seul `resize_agent` au PTY.
  $effect(() => {
    if (!panelDrag.active && activeId) refit(activeId);
  });

  // Un autre agent : sa recherche repart de zéro.
  $effect(() => {
    void activeId;
    finding = false;
  });

  /**
   * Fichiers déposés depuis le Finder : la webview n'en voit pas les chemins, Tauri si. Le dépôt
   * vise le terminal sous le curseur, quel que soit l'agent actif.
   */
  $effect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === 'leave') return void (dropping = false);
      const { x, y } = payload.position.toLogical(window.devicePixelRatio);
      const target = terminalAt(x, y);
      dropping = payload.type !== 'drop' && target !== undefined;
      if (payload.type === 'drop' && target && !dropPaths(target, payload.paths)) {
        app.notify('Dépôt refusé : un nom de fichier contient un caractère de contrôle (retour à la ligne…).');
      }
    });
    return () => void unlisten.then((stop) => stop());
  });

  function onkeydown(e: KeyboardEvent) {
    if (!active || viewed || linked) return;
    const shortcut = terminalShortcut(e);
    if (shortcut === 'find') {
      finding = true;
      search?.focus();
    } else if (shortcut === 'full') terminalFull.toggle();
    else if (shortcut) resizeFont(shortcut);
    else return;
    e.preventDefault();
  }

  $effect(() => {
    if (active?.status === 'awaitingInput') void api.markAgentSeen(active.id);
  });

  /** Arrêt puis reprise de la même session : la conversation continue sur le binaire à jour. */
  let confirmRestart = $state(false);
  async function restart() {
    if (!active) return;
    const busy = active.status === 'running' || active.status === 'starting';
    if (busy && !confirmRestart) {
      confirmRestart = true;
      app.notify('L’agent travaille : son tour sera interrompu. Clique à nouveau sur Redémarrer pour confirmer.');
      setTimeout(() => (confirmRestart = false), 5000);
      return;
    }
    confirmRestart = false;
    try {
      const fresh = await api.restartAgent(active.id, 120, 36);
      app.openAgent(fresh.id);
    } catch (e) {
      app.notify(String(e));
    }
  }

  /** Identifiant de la session Claude Code, celui que `claude --resume` attend. */
  async function copySessionId(id: string) {
    try {
      await navigator.clipboard.writeText(id);
      app.notify('Id de session copié');
    } catch (e) {
      app.notify(`Copie impossible : ${e}`);
    }
  }

  const fullLabel = $derived(
    terminalFull.shown
      ? `Quitter la pleine fenêtre${app.signal ? ` · ${app.signal.text}` : ''}`
      : 'Terminal en pleine fenêtre',
  );

  async function stop() {
    if (active) await api.killAgent(active.id).catch((e) => app.notify(String(e)));
  }
</script>

<svelte:window {onkeydown} />

<section class="page" class:full={terminalFull.shown}>
  <header class="page-head" data-tauri-drag-region>
    <h1>Agents</h1>
    <span class="muted">Sessions lancées dans Robiade</span>
    <span class="spacer"></span>
    <button class="btn" onclick={() => (app.resumeOpen = true)} title="Toutes les sessions, comme /resume">
      <Icon name="resume" size={12} /> Reprendre… <kbd>⌘O</kbd>
    </button>
    <button class="btn primary" onclick={() => app.newAgent()}><Icon name="plus" size={14} /> Nouvel agent</button>
  </header>

  {#if app.visibleAgents.length === 0 && recent.length === 0}
    <div class="empty">
      <div>
        <strong>Aucun agent pour l'instant.</strong>
        Lance une session Claude Code dans un projet avec <kbd>⌘N</kbd>, ou reprends-en une depuis l'historique.
      </div>
    </div>
  {:else}
    <div
      class="split"
      bind:clientWidth={splitW}
      style:--rail-w={railWidth ? `${railWidth}px` : null}
      style:--side-w={sideWidth ? `${sideWidth}px` : null}
    >
      <!-- Liseré à la couleur du projet (--tab-project) : il se lit sans lire le texte. -->
      {#snippet tab(a: Agent)}
        <li class="tab" style:--tab-project={app.colorVar(a.projectRoot)}>
          <button
            class="agent live"
            class:active={active?.id === a.id}
            onclick={() => app.openAgent(a.id)}
            ondblclick={() => startRename(a.id)}
            title="Double-clic pour renommer"
          >
            <div class="line">
              <StatusBadge status={a.status} compact />
              <strong class="ellipsis">{a.name ?? a.projectName}</strong>
              <span class="muted num right">{ago(a.lastActivityAt, app.now)}</span>
            </div>
            <div class="line sub">
              {#if a.name}<span class="muted ellipsis">{a.projectName}</span>{/if}
              {#if a.worktreeName}<span class="mono muted ellipsis wt">{a.worktreeName}</span>{/if}
              {#if a.status !== 'ended'}
                <StatusBadge status={a.status} />
                <ModeBadge mode={a.permissionMode} permissive={a.permissive} compact />
                {#if a.lastTool}<span class="mono muted ellipsis">· {a.lastTool}</span>{/if}
              {/if}
            </div>
          </button>
          {#if a.status === 'ended'}
            <button class="close" onclick={() => forget(a.id)} title="Fermer l'onglet (la session reste dans l'historique)">
              <Icon name="close" size={12} />
            </button>
          {/if}
        </li>
      {/snippet}
      <!-- Les agents en cours prennent la colonne ; les récents restent ancrés en bas, hauteur bornée. -->
      <div class="rail" bind:offsetWidth={railW}>
        <ResizeHandle
          id="agents.list"
          edge="right"
          min={180}
          max={splitW ? splitW - SIDE_MIN - TERMINAL_MIN : Infinity}
          label="Largeur de la liste des agents"
          bind:width={railWidth}
        />
        <ul class="list running">
          <li class="group section-title">En cours <span class="num">{running.length}</span></li>
          {#each running as a (a.id)}{@render tab(a)}{:else}<li class="none muted">Aucun agent en cours</li>{/each}
        </ul>
        <div class="recents">
          {#if recent.length}
            <h2 class="group section-title">Récents · à reprendre</h2>
            <ul class="list">
              {#each recent as r (r.kind === 'tab' ? r.agent.id : r.session.sessionId)}
                {#if r.kind === 'tab'}
                  <li class="tab" style:--tab-project={app.colorVar(r.agent.projectRoot)}>
                    <button
                      class="recent"
                      class:active={active?.id === r.agent.id}
                      onclick={() => app.openAgent(r.agent.id)}
                      title={[r.agent.projectName, r.agent.worktreeName].filter(Boolean).join(' · ')}
                    >
                      <StatusBadge status={r.agent.status} compact />
                      <span class="ellipsis">{r.agent.name ?? r.agent.projectName}</span>
                      <span class="num right">{ago(r.at, app.now)}</span>
                    </button>
                    <button class="close" onclick={() => forget(r.agent.id)} title="Fermer l'onglet (la session reste dans l'historique)">
                      <Icon name="close" size={12} />
                    </button>
                  </li>
                {:else}
                  <li style:--tab-project={app.colorVar(r.session.projectRoot)}>
                    <button
                      class="recent"
                      onclick={() => app.spawn(r.session.projectPath, r.session.sessionId)}
                      title={`Reprendre dans un nouvel onglet — ${[r.session.projectName, r.session.worktreeName].filter(Boolean).join(' · ')}`}
                    >
                      <Icon name="resume" size={11} />
                      <span class="ellipsis">{r.session.title}</span>
                      <span class="num right">{ago(r.at, app.now)}</span>
                    </button>
                  </li>
                {/if}
              {/each}
            </ul>
          {/if}
          <button class="more" onclick={() => (app.resumeOpen = true)}>Toutes les sessions… <kbd>⌘O</kbd></button>
        </div>
      </div>

      {#if active}
        <div class="term-wrap" style:--tab-project={app.colorVar(active.projectRoot)}>
          <!-- En pleine fenêtre, la barre passe sous les feux tricolores : elle déplace la fenêtre. -->
          <div class="term-bar" data-tauri-drag-region={terminalFull.shown || null}>
            {#if renaming && draft}
              <input
                class="field name-field"
                bind:this={nameField}
                bind:value={draft.value}
                onkeydown={onNameKey}
                onblur={saveName}
                placeholder={active.projectName}
                maxlength="60"
                aria-label="Nom de l'onglet"
              />
            {:else}
              <button class="name" onclick={() => startRename(active.id)} title="Renommer l'onglet">
                <span class="ellipsis">{active.name ?? 'Sans nom'}</span><Icon name="pencil" size={12} />
              </button>
            {/if}
            <StatusBadge status={active.status} />
            <ModeBadge mode={active.permissionMode} permissive={active.permissive} />
            <span class="path mono muted ellipsis">{shortPath(active.projectPath, app.env?.home ?? '')}</span>
            {#if session?.gitBranch}
              <!-- Coupée par le début : la fin distingue les branches. `bdi` garde les `/` en place dans le `rtl`. -->
              <span class="branch mono" title={session.gitBranch}>
                <Icon name="branch" size={12} /><span class="tail" dir="rtl"><bdi>{session.gitBranch}</bdi></span>
              </span>
            {/if}
            {#if mr}<MrBadge url={mr.url} state={mr.state} title={mr.title} />{/if}
            <span class="spacer" data-tauri-drag-region={terminalFull.shown || null}></span>
            {#if active.status !== 'ended'}
              <button class="btn ghost" onclick={restart} title="Relance claude sur la même session : prend une mise à jour installée (« Restart to update »)">
                <Icon name="resume" size={12} /><span class="label">Redémarrer</span>
              </button>
              <button class="btn ghost" onclick={stop} title="Arrêter le process"><Icon name="stop" size={13} /><span class="label">Arrêter</span></button>
            {:else}
              <button class="btn" onclick={() => app.spawn(active.projectPath, active.sessionId)} title="Reprendre la session">
                <Icon name="resume" size={12} /><span class="label">Reprendre</span>
              </button>
            {/if}
            <!-- Liste cachée, le bouton porte son signal : un agent qui attend ne disparaît pas. -->
            <button
              class="btn ghost full-toggle"
              onclick={terminalFull.toggle}
              aria-pressed={terminalFull.shown}
              aria-label={fullLabel}
              title="{fullLabel} (⌘⇧↩)"
            >
              <Icon name={terminalFull.shown ? 'restore' : 'maximize'} size={13} />
              {#if terminalFull.shown && app.signal}<span class="signal" class:hot={app.signal.hot}>{app.signal.count}</span>{/if}
            </button>
          </div>
          <GaugeBar gauges={active.gauges} />
          <div class="term-area" class:dropping>
            <div class="term" bind:this={container}></div>
            {#if finding}
              <TerminalSearch bind:this={search} id={active.id} onclose={() => (finding = false)} />
            {/if}
          </div>
          <div class="term-foot">
            <LifecycleBar agent={active} bind:lifecycle />
            <ShortcutBar agent={active} />
          </div>
        </div>

        <!-- Enveloppe sans défilement : la poignée reste sur la bordure quand le panneau défile. -->
        <div class="side">
          <ResizeHandle
            id="agents.side"
            edge="left"
            min={SIDE_MIN}
            max={splitW ? splitW - railW - TERMINAL_MIN : Infinity}
            label="Largeur du panneau Session et Diff"
            bind:width={sideWidth}
          />
          <aside class="stats">
            <div class="side-tabs" role="tablist">
              <button role="tab" aria-selected={side === 'session'} class:on={side === 'session'} onclick={() => (side = 'session')}>
                Session
              </button>
              <button role="tab" aria-selected={side === 'diff'} class:on={side === 'diff'} onclick={() => (side = 'diff')}>
                Diff
              </button>
            </div>
            {#if side === 'diff'}
              <ChangesPanel agentId={active.id} activity={active.lastActivityAt + active.filesModified} />
            {:else}
            <dl>
              <dt>Session</dt>
              <dd>
                <button class="session-id mono" onclick={() => copySessionId(sessionId)} title="{sessionId} — cliquer pour copier">
                  {sessionId.slice(0, 8)}<Icon name="copy" size={11} />
                </button>
              </dd>
              <dt>Durée</dt><dd class="num">{duration(app.now - active.startedAt)}</dd>
              <dt>T'a attendu</dt><dd class="num">{duration(active.waitedMs)}</dd>
              <dt>dont permission</dt><dd class="num">{duration(active.permissionMs)}</dd>
              <dt>Appels d'outils</dt><dd class="num">{active.toolCalls}</dd>
              <dt>Sous-agents</dt><dd class="num">{active.subagents}</dd>
              <dt>Fichiers modifiés</dt><dd class="num">{active.filesModified}</dd>
              {#if session}
                <dt>Tokens</dt><dd class="num">{tokens(session.tokens.total)}</dd>
                <dt>entrée</dt><dd class="num">{tokens(session.tokens.input)}</dd>
                <dt>sortie</dt><dd class="num">{tokens(session.tokens.output)}</dd>
                <dt>cache</dt><dd class="num">{tokens(session.tokens.cacheRead + session.tokens.cacheCreation)}</dd>
                <dt title="Tokens × tarif public de l'API ; avec un abonnement, ce n'est pas ce que tu paies">Coût estimé</dt>
                <dd class="num">{dollars(session.cost.dollars)}</dd>
                <dt>Messages</dt><dd class="num">{session.userMessages} / {session.assistantMessages}</dd>
              {/if}
              <LaunchPicker agent={active} />
              <dt>Contexte</dt><dd class="num">{active.gauges.context ? `${active.gauges.context.percent} %` : '—'}</dd>
            </dl>
            {#if !session}
              <p class="muted note">Tokens et modèle apparaissent dès que le transcript existe (après le premier échange).</p>
            {/if}

            {#if active.tools.length}
              <h2 class="section-title assets-title">Outils</h2>
              <dl class="tools">
                {#each active.tools.slice(0, 10) as [tool, n] (tool)}
                  <dt class="mono ellipsis">{tool}</dt><dd class="num">{n}</dd>
                {/each}
              </dl>
            {/if}

            <h2 class="section-title assets-title">Assets {#if active.assets.length}<span class="num">{active.assets.length}</span>{/if}</h2>
            {#if active.assets.length === 0}
              <p class="muted note">Images, pages HTML et notes que l'agent écrit apparaissent ici.</p>
            {:else}
              <ul class="assets">
                {#each active.assets as f (f.path)}
                  <li>
                    <button class="asset" onclick={() => (viewing = f.path)} title={f.path}>
                      <Icon name={f.kind === 'image' ? 'image' : 'file'} size={14} />
                      <span class="ellipsis">{f.name}</span>
                      <span class="muted num right">{ago(f.writtenAt, app.now)}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
            {/if}
          </aside>
        </div>
      {:else}
        <div class="empty pick">
          <div><strong>Aucun agent ouvert.</strong>Reprends une session récente à gauche, ou lance-en une avec <kbd>⌘N</kbd>.</div>
        </div>
      {/if}
    </div>
  {/if}
</section>

{#if active && viewed}
  {@const id = active.id}
  <AssetViewer
    name={viewed.name}
    path={viewed.path}
    load={(path) => api.readAsset(id, path).then(viewable)}
    assets={active.assets}
    onnavigate={(path) => (viewing = path)}
    onclose={() => (viewing = null)}
  />
{:else if active && linked?.agentId === active.id}
  {@const { agentId, uri, line } = linked}
  {@const path = linkedPath(uri)}
  <AssetViewer name={baseName(path)} {path} {line} load={() => readLinkedFile(agentId, uri)} onclose={() => (linked = null)} />
{/if}

<style>
  .session-id {
    display: inline-flex; align-items: center; gap: 4px; padding: 0 4px; margin-right: -4px;
    border: 0; border-radius: 4px; background: none; color: var(--ink-2); font-size: 12px;
  }
  .session-id:hover { background: var(--bg-active); color: var(--ink); }
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  /* Masqués, pas démontés : largeurs réglées et état des listes reviennent tels quels. */
  .full .page-head, .full .rail, .full .side { display: none; }
  .full .split { grid-template-columns: 1fr; }
  .full .term-bar { padding-left: var(--titlebar-inset); }
  .split {
    display: grid; grid-template-columns: var(--rail-w, 250px) 1fr var(--side-w, 260px); min-height: 0; flex: 1;
  }
  .rail { position: relative; display: flex; flex-direction: column; min-height: 0; border-right: 1px solid var(--line); }
  .list { list-style: none; margin: 0; padding: 0; }
  .running { flex: 1; min-height: 0; overflow-y: auto; padding: 8px; }
  /* Hauteur bornée : seule la liste défile, le titre et « Toutes les sessions… » restent visibles. */
  .recents { display: flex; flex-direction: column; flex: none; max-height: 40%; padding: 8px; border-top: 1px solid var(--line); }
  .recents .list { min-height: 0; overflow-y: auto; }
  /* En retrait des agents ouverts : fond enfoncé, pastilles atténuées jusqu'au survol. */
  .recents { background: var(--bg-sunken); }
  .recent :global(.badge) { opacity: 0.55; }
  .recent:hover :global(.badge), .recent.active :global(.badge) { opacity: 1; }
  .none { padding: 4px 10px; font-size: 12px; }
  .agent {
    display: flex; flex-direction: column; gap: 2px; width: 100%; padding: 8px 10px;
    border: 0; border-radius: var(--radius-sm); background: none; text-align: left;
  }
  .agent:hover { background: var(--bg-hover); }
  .tab { position: relative; }
  .close {
    position: absolute; top: 7px; right: 6px; display: none; padding: 3px; border: 0; border-radius: 4px;
    background: var(--bg-raised); color: var(--ink-3);
  }
  .tab:hover .close { display: grid; }
  .close:hover { color: var(--ink); background: var(--bg-hover); }
  .more {
    display: flex; align-items: center; justify-content: space-between; width: 100%; margin-top: 6px;
    padding: 6px 10px; border: 1px dashed var(--line-strong); border-radius: var(--radius-sm);
    background: none; color: var(--ink-2); font-size: 12.5px;
  }
  .more:hover { color: var(--ink); background: var(--bg-hover); }
  .group { padding: 4px 10px; }
  .group .num { margin-left: 4px; color: var(--ink-2); }
  /* Couleur du projet, transparente sans couleur : l'accent reste à ce qui attend. */
  .agent.live { border-left: 3px solid var(--tab-project, transparent); border-radius: 0 var(--radius-sm) var(--radius-sm) 0; }
  .recent {
    display: flex; align-items: center; gap: 6px; width: 100%; padding: 3px 10px;
    border: 0; border-radius: var(--radius-sm); background: none; text-align: left;
    color: var(--ink-3); font-size: 12px;
    box-shadow: inset 3px 0 0 var(--tab-project, transparent);
  }
  li[style] > .recent { border-radius: 0 var(--radius-sm) var(--radius-sm) 0; }
  .recent:hover { color: var(--ink); background: var(--bg-hover); }
  .recent.active { color: var(--ink); background: var(--bg-active); }
  .tab:has(.recent) .close { top: 2px; }
  .pick { grid-column: 2 / 4; }
  .name {
    display: inline-flex; align-items: center; gap: 6px; min-width: 0; max-width: 220px; padding: 2px 6px; margin-left: -6px;
    border: 0; border-radius: var(--radius-sm); background: none; color: var(--ink); font-weight: 600;
  }
  .name :global(svg) { color: var(--ink-3); opacity: 0; }
  .name:hover { background: var(--bg-hover); }
  .name:hover :global(svg) { opacity: 1; }
  .name-field { width: 220px; min-width: 0; height: 26px; }
  .agent.active { background: var(--bg-active); }
  .line { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .sub { padding-left: 20px; }
  .sub :global(.badge svg) { display: none; }
  .wt { font-size: 11.5px; }
  .right { margin-left: auto; flex: none; white-space: nowrap; font-size: 11.5px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .term-wrap { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--bg-sunken); }
  .term-bar {
    display: flex; align-items: center; gap: 12px; height: 40px; padding: 0 12px; flex: none;
    border-bottom: 1px solid var(--line); background: var(--bg); container-type: inline-size;
    box-shadow: inset 3px 0 0 var(--tab-project, transparent);
  }
  .spacer { flex: 1; }
  /* Quand la place manque, le chemin cède d'abord, puis branche et titre de MR, le nom en dernier ; les actions jamais. */
  .term-bar .btn { flex: none; }
  .path { flex-shrink: 100; }
  .name { flex-shrink: 0.25; }
  /*
   * Barre étroite : les actions passent en icône seule, l'état en pastille seule, le mode permissif
   * en triangle seul ; un mode non permissif disparaît. Les libellés restent au survol.
   */
  @container (max-width: 720px) {
    .term-bar .btn .label,
    .term-bar :global(.compact-hide) { display: none; }
  }
  .full-toggle { position: relative; }
  /* Même pastille que le bouton de la barre latérale repliée. */
  .signal {
    position: absolute; top: -4px; right: -6px; min-width: 15px; height: 15px; padding: 0 4px;
    border-radius: 8px; background: var(--info); box-shadow: 0 0 0 2px var(--bg);
    color: #fff; font-size: 10px; font-weight: 650; line-height: 15px; text-align: center;
    font-variant-numeric: tabular-nums;
  }
  .signal.hot { background: var(--accent); }
  .branch { display: inline-flex; align-items: center; gap: 4px; min-width: 0; color: var(--ink-2); }
  .branch :global(svg) { flex: none; }
  .tail { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .term-area { flex: 1; min-height: 0; display: flex; position: relative; }
  /* Frise du ticket et raccourcis, sur une ligne fixe ; leurs container queries se mesurent sur elle. */
  .term-foot {
    flex: none; display: flex; align-items: center; gap: 12px; height: 30px; padding: 0 8px 0 14px;
    border-top: 1px solid var(--line); background: var(--bg); font-size: 12px; container-type: inline-size;
  }
  .term-foot:not(:has(*)) { display: none; }
  .term-area.dropping { outline: 2px solid var(--accent); outline-offset: -2px; }
  /* `fit` suspendu pendant un glisser : xterm garde sa taille, rognée plutôt que débordante. */
  .term { flex: 1; min-width: 0; min-height: 0; overflow: hidden; padding: 10px 0 4px 12px; background: #24273a; /* TERMINAL_THEME.background */ }
  .term :global(.term-host) { height: 100%; }
  .side { position: relative; display: flex; min-width: 0; min-height: 0; }
  .stats { flex: 1; min-width: 0; border-left: 1px solid var(--line); padding: 12px 16px 16px; overflow-y: auto; }
  .side-tabs { display: flex; gap: 2px; margin: 0 -6px 12px; }
  .side-tabs button {
    flex: 1; height: 26px; border: 0; border-radius: var(--radius-sm); background: none; color: var(--ink-3); font-size: 12px;
  }
  .side-tabs button:hover { color: var(--ink); background: var(--bg-hover); }
  .side-tabs button.on { color: var(--ink); background: var(--bg-active); font-weight: 550; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 8px 12px; margin: 0; }
  dt { color: var(--ink-3); }
  dd { margin: 0; text-align: right; min-width: 0; }
  .note { font-size: 11.5px; margin: 16px 0 0; }
  .assets-title { margin-top: 24px; display: flex; gap: 6px; }
  .tools { margin-top: 6px; gap: 4px 12px; font-size: 12px; }
  .tools dt { min-width: 0; }
  .assets { list-style: none; margin: 6px -6px 0; padding: 0; }
  .asset {
    display: flex; align-items: center; gap: 8px; width: 100%; height: 28px; padding: 0 6px;
    border: 0; border-radius: var(--radius-sm); background: none; color: var(--ink-2); text-align: left;
  }
  .asset:hover { background: var(--bg-hover); color: var(--ink); }
</style>
