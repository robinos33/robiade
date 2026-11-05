<script lang="ts">
  import { toggleSidebar } from '../lib/sidebar.svelte';
  import { terminalFull } from '../lib/terminalFull.svelte';
  import { app, type View } from '../lib/state.svelte';

  interface Command { label: string; hint: string; run: () => void }

  let query = $state('');
  let index = $state(0);
  let input: HTMLInputElement | undefined = $state();

  const VIEWS: [View, string][] = [
    ['inbox', 'Inbox'], ['agents', 'Agents'], ['history', 'Historique'], ['tasks', 'Tâches'], ['stats', 'Statistiques'],
    ['env', 'Environnements'], ['db', 'Bases de données'], ['code', 'Code'], ['settings', 'Réglages'],
  ];

  const commands = $derived.by<Command[]>(() => {
    const all: Command[] = [
      { label: 'Nouvel agent', hint: '⌘N · ⌘T', run: () => app.newAgent() },
      { label: 'Nouvel agent dans…', hint: '⇧⌘N', run: () => (app.newAgentOpen = true) },
      { label: 'Reprendre une session…', hint: '⌘O', run: () => (app.resumeOpen = true) },
      { label: 'Aller au fichier…', hint: '⌘P', run: () => app.findFile() },
      { label: 'Afficher / masquer la barre latérale', hint: '⌘B', run: toggleSidebar },
      {
        label: 'Terminal en pleine fenêtre / disposition normale',
        hint: '⌘⇧↩',
        run: () => {
          app.view = 'agents';
          terminalFull.toggle();
        },
      },
      ...VIEWS.map(([v, label]) => ({ label: `Aller à ${label}`, hint: 'vue', run: () => (app.view = v) })),
      { label: 'Vue globale', hint: 'projet', run: () => (app.project = null) },
      ...app.projects.map((p) => ({ label: p.name, hint: 'projet', run: () => (app.project = p.root) })),
      ...app.agents.map((a) => ({ label: `Agent · ${a.projectName}`, hint: a.status, run: () => app.openAgent(a.id) })),
      ...app.conversations.slice(0, 300).map((s) => ({ label: s.title, hint: s.projectName, run: () => app.openSession(s.sessionId) })),
    ];
    const q = query.trim().toLowerCase();
    return (q ? all.filter((c) => `${c.label} ${c.hint}`.toLowerCase().includes(q)) : all).slice(0, 40);
  });

  $effect(() => input?.focus());

  function run(c: Command | undefined) {
    if (!c) return;
    app.paletteOpen = false;
    c.run();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') index = Math.min(index + 1, commands.length - 1);
    else if (e.key === 'ArrowUp') index = Math.max(index - 1, 0);
    else if (e.key === 'Enter') run(commands[index]);
    else if (e.key === 'Escape') app.paletteOpen = false;
    else return;
    e.preventDefault();
  }
</script>

<div class="scrim" role="presentation" onclick={() => (app.paletteOpen = false)}>
  <div class="dialog" role="dialog" aria-label="Palette de commandes" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
    <input placeholder="Projet, session, vue, action…" bind:value={query} bind:this={input} {onkeydown} oninput={() => (index = 0)} />
    <ul>
      {#each commands as c, i (i)}
        <li>
          <button class:active={i === index} onmouseenter={() => (index = i)} onclick={() => run(c)}>
            <span class="label">{c.label}</span><span class="hint">{c.hint}</span>
          </button>
        </li>
      {:else}
        <li class="none">Rien ne correspond.</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 60; display: grid; justify-items: center; align-items: start; padding-top: 12vh; background: rgb(0 0 0 / 0.3); }
  .dialog { width: min(620px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow); overflow: hidden; outline: none; }
  input { width: 100%; height: 48px; padding: 0 16px; border: 0; border-bottom: 1px solid var(--line); background: none; font-size: 15px; outline: none; }
  ul { list-style: none; margin: 0; padding: 6px; max-height: 380px; overflow-y: auto; }
  button { display: flex; align-items: center; gap: 12px; width: 100%; height: 34px; padding: 0 10px; border: 0; border-radius: 6px; background: none; text-align: left; }
  button.active { background: var(--bg-active); }
  .label { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hint { color: var(--ink-3); font-size: 12px; flex: none; }
  .none { padding: 10px; color: var(--ink-3); }
</style>
