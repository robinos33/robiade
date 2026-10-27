<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { attach, refit } from '../lib/terminals';
  import { ago, duration, shortPath, tokens } from '../lib/format';
  import StatusBadge from '../components/StatusBadge.svelte';
  import MrBadge from '../components/MrBadge.svelte';
  import ModeBadge from '../components/ModeBadge.svelte';
  import GaugeBar from '../components/GaugeBar.svelte';
  import Icon from '../components/Icon.svelte';

  let container: HTMLDivElement | undefined = $state();

  const active = $derived(app.agents.find((a) => a.id === app.activeAgentId) ?? app.visibleAgents[0]);
  const session = $derived(active ? app.sessionOf(active) : undefined);

  $effect(() => {
    if (!active || !container) return;
    attach(active.id, container);
    if (active.status === 'awaitingInput') void api.markAgentSeen(active.id);
    const observer = new ResizeObserver(() => refit(active.id));
    observer.observe(container);
    return () => observer.disconnect();
  });

  async function stop() {
    if (active) await api.killAgent(active.id).catch((e) => app.notify(String(e)));
  }
</script>

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Agents</h1>
    <span class="muted">Sessions lancées dans Robiade</span>
    <span class="spacer"></span>
    <button class="btn primary" onclick={() => (app.newAgentOpen = true)}><Icon name="plus" size={14} /> Nouvel agent</button>
  </header>

  {#if app.visibleAgents.length === 0}
    <div class="empty">
      <div>
        <strong>Aucun agent pour l'instant.</strong>
        Lance une session Claude Code dans un projet avec <kbd>⌘N</kbd>, ou reprends-en une depuis l'historique.
      </div>
    </div>
  {:else}
    <div class="split">
      <ul class="list">
        {#each app.visibleAgents as a (a.id)}
          <li>
            <button class="agent" class:active={active?.id === a.id} onclick={() => app.openAgent(a.id)}>
              <div class="line">
                <StatusBadge status={a.status} compact />
                <strong class="ellipsis">{a.projectName}</strong>
                {#if a.worktreeName}<span class="mono muted ellipsis wt">{a.worktreeName}</span>{/if}
                <span class="muted num right">{ago(a.lastActivityAt, app.now)}</span>
              </div>
              <div class="line sub">
                <StatusBadge status={a.status} />
                <ModeBadge mode={a.permissionMode} permissive={a.permissive} compact />
                {#if a.lastTool}<span class="mono muted ellipsis">· {a.lastTool}</span>{/if}
              </div>
            </button>
          </li>
        {/each}
      </ul>

      {#if active}
        <div class="term-wrap">
          <div class="term-bar">
            <StatusBadge status={active.status} />
            <ModeBadge mode={active.permissionMode} permissive={active.permissive} />
            <span class="mono muted ellipsis">{shortPath(active.projectPath, app.env?.home ?? '')}</span>
            {#if session?.gitBranch}
              <span class="branch mono"><Icon name="branch" size={12} /> {session.gitBranch}</span>
            {/if}
            {#if session?.mergeRequestUrl}<MrBadge url={session.mergeRequestUrl} />{/if}
            <span class="spacer"></span>
            {#if active.status !== 'ended'}
              <button class="btn ghost" onclick={stop} title="Arrêter le process"><Icon name="stop" size={13} /> Arrêter</button>
            {:else}
              <button class="btn" onclick={() => app.spawn(active.projectPath, active.resumedFrom ?? active.id)}>
                <Icon name="resume" size={12} /> Reprendre
              </button>
            {/if}
          </div>
          <GaugeBar gauges={active.gauges} />
          <div class="term" bind:this={container}></div>
        </div>

        <aside class="stats">
          <h2 class="section-title">Session en cours</h2>
          <dl>
            <dt>Durée</dt><dd class="num">{duration(app.now - active.startedAt)}</dd>
            <dt>T'a attendu</dt><dd class="num">{duration(active.waitedMs)}</dd>
            <dt>Appels d'outils</dt><dd class="num">{active.toolCalls}</dd>
            <dt>Dernier outil</dt><dd class="mono">{active.lastTool ?? '—'}</dd>
            {#if session}
              <dt>Tokens</dt><dd class="num">{tokens(session.tokens.total)}</dd>
              <dt>dont sortie</dt><dd class="num">{tokens(session.tokens.output)}</dd>
              <dt>Messages</dt><dd class="num">{session.userMessages} / {session.assistantMessages}</dd>
              <dt>Modèle</dt><dd class="mono ellipsis">{active.gauges.model ?? session.models.at(-1) ?? '—'}</dd>
            {/if}
            <dt>Contexte</dt><dd class="num">{active.gauges.context ? `${active.gauges.context.percent} %` : '—'}</dd>
          </dl>
          {#if !session}
            <p class="muted note">Tokens et modèle apparaissent dès que le transcript existe (après le premier échange).</p>
          {/if}
        </aside>
      {/if}
    </div>
  {/if}
</section>

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .split { display: grid; grid-template-columns: 250px 1fr 220px; min-height: 0; flex: 1; }
  .list { list-style: none; margin: 0; padding: 8px; overflow-y: auto; border-right: 1px solid var(--line); }
  .agent {
    display: flex; flex-direction: column; gap: 2px; width: 100%; padding: 8px 10px;
    border: 0; border-radius: var(--radius-sm); background: none; text-align: left;
  }
  .agent:hover { background: var(--bg-hover); }
  .agent.active { background: var(--bg-active); }
  .line { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .sub { padding-left: 20px; }
  .sub :global(.badge svg) { display: none; }
  .wt { font-size: 11.5px; }
  .right { margin-left: auto; font-size: 11.5px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .term-wrap { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--bg-sunken); }
  .term-bar {
    display: flex; align-items: center; gap: 12px; height: 40px; padding: 0 12px; flex: none;
    border-bottom: 1px solid var(--line); background: var(--bg);
  }
  .spacer { flex: 1; }
  .branch { display: inline-flex; align-items: center; gap: 4px; color: var(--ink-2); }
  .term { flex: 1; min-height: 0; padding: 10px 0 4px 12px; }
  .term :global(.term-host) { height: 100%; }
  .stats { border-left: 1px solid var(--line); padding: 16px; overflow-y: auto; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 8px 12px; margin: 0; }
  dt { color: var(--ink-3); }
  dd { margin: 0; text-align: right; min-width: 0; }
  .note { font-size: 11.5px; margin: 16px 0 0; }
</style>
