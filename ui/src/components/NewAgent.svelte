<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { shortPath } from '../lib/format';
  import Icon from './Icon.svelte';

  let query = $state('');
  let index = $state(0);
  let input: HTMLInputElement | undefined = $state();

  // Projets puis worktrees : un agent se lance aussi bien dans l'un que dans l'autre.
  const targets = $derived([
    ...app.projects.map((p) => ({ path: p.root, root: p.root, name: p.name, icon: 'folder', of: null as string | null })),
    ...app.worktrees.map((w) => ({ path: w.path, root: w.root, name: w.name, icon: 'branch', of: w.projectName })),
  ]);
  const choices = $derived(
    targets.filter((t) => `${t.name} ${t.of ?? ''} ${t.path}`.toLowerCase().includes(query.toLowerCase())).slice(0, 12),
  );

  $effect(() => {
    input?.focus();
    const current = targets.findIndex((t) => t.path === (app.worktree ?? app.project));
    index = Math.max(0, current);
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') index = Math.min(index + 1, choices.length - 1);
    else if (e.key === 'ArrowUp') index = Math.max(index - 1, 0);
    else if (e.key === 'Enter') {
      const choice = choices[index];
      if (choice) void app.spawn(choice.path);
    }
    else if (e.key === 'Escape') app.newAgentOpen = false;
    else return;
    e.preventDefault();
  }
</script>

<div class="scrim" role="presentation" onclick={() => (app.newAgentOpen = false)}>
  <div class="dialog" role="dialog" aria-label="Nouvel agent" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
    <header>
      <h2>Nouvel agent</h2>
      <p class="muted">Lance <span class="mono">claude</span> dans le projet choisi, dans un terminal intégré.</p>
    </header>
    <input class="field" placeholder="Projet ou worktree…" bind:value={query} bind:this={input} {onkeydown} oninput={() => (index = 0)} />
    <ul>
      {#each choices as t, i (t.path)}
        <li>
          <button class:active={i === index} onmouseenter={() => (index = i)} onclick={() => app.spawn(t.path)}>
            <!-- Comme dans la barre latérale : l'icône porte la couleur du projet. -->
            <span class="ico" style:color={app.colorVar(t.root)}><Icon name={t.icon} size={14} /></span>
            <span class="name">{t.name}</span>
            {#if t.of}<span class="muted">{t.of}</span>{/if}
            <span class="path mono muted">{shortPath(t.path, app.env?.home ?? '')}</span>
          </button>
        </li>
      {:else}
        <li class="muted none">Aucun projet ni worktree connu ne correspond.</li>
      {/each}
    </ul>
    {#if !app.env?.claudePath}
      <p class="warn">Binaire <span class="mono">claude</span> introuvable dans le PATH du shell de login.</p>
    {/if}
    <footer class="muted"><kbd>↑↓</kbd> choisir <kbd>↵</kbd> lancer <kbd>esc</kbd> fermer</footer>
  </div>
</div>

<style>
  .ico { display: grid; flex: none; }
  .scrim {
    position: fixed; inset: 0; z-index: 50; display: grid; justify-items: center; align-items: start;
    padding-top: 14vh; background: rgb(0 0 0 / 0.35); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(560px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 16px; display: flex; flex-direction: column; gap: 12px; outline: none;
  }
  h2 { margin: 0 0 2px; font-size: 15px; }
  p { margin: 0; }
  ul { list-style: none; margin: 0; padding: 0; max-height: 340px; overflow-y: auto; }
  li button {
    display: flex; align-items: center; gap: 10px; width: 100%; height: 34px; padding: 0 10px;
    border: 0; border-radius: 6px; background: none; text-align: left; color: var(--ink-2);
  }
  li button.active { background: var(--bg-active); color: var(--ink); }
  .name { font-weight: 550; }
  .path { margin-left: auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 60%; }
  .none { padding: 10px; }
  .warn { color: var(--danger); }
  footer { display: flex; gap: 6px; align-items: center; font-size: 12px; }
  footer kbd { margin-left: 8px; }
  footer kbd:first-child { margin-left: 0; }
</style>
