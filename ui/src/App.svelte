<script lang="ts">
  import { onMount } from 'svelte';
  import { app, type View } from './lib/state.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import NewAgent from './components/NewAgent.svelte';
  import Palette from './components/Palette.svelte';
  import QuitConfirm from './components/QuitConfirm.svelte';
  import Inbox from './views/Inbox.svelte';
  import Agents from './views/Agents.svelte';
  import History from './views/History.svelte';
  import MergeRequests from './views/MergeRequests.svelte';
  import Tasks from './views/Tasks.svelte';
  import Stats from './views/Stats.svelte';

  const KEYS: Record<string, View> = { '1': 'inbox', '2': 'agents', '3': 'history', '4': 'tasks', '5': 'stats', '6': 'mr' };

  onMount(() => {
    app.init().catch((e) => app.notify(`Démarrage incomplet : ${e}`));
  });

  function onkeydown(e: KeyboardEvent) {
    if (!e.metaKey) return;
    const view = KEYS[e.key];
    if (view) app.view = view;
    else if (e.key === 'k') app.paletteOpen = !app.paletteOpen;
    else if (e.key === 'n') app.newAgentOpen = true;
    else return;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<div class="shell">
  <Sidebar />
  <main>
    {#if app.view === 'inbox'}<Inbox />
    {:else if app.view === 'agents'}<Agents />
    {:else if app.view === 'history'}<History />
    {:else if app.view === 'mr'}<MergeRequests />
    {:else if app.view === 'tasks'}<Tasks />
    {:else}<Stats />{/if}
  </main>
</div>

{#if app.newAgentOpen}<NewAgent />{/if}
{#if app.paletteOpen}<Palette />{/if}
{#if app.quitRequest}<QuitConfirm />{/if}
{#if app.toast}<div class="toast" role="status">{app.toast}</div>{/if}

<style>
  .shell { display: flex; height: 100%; }
  main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .toast {
    position: fixed; bottom: 18px; left: 50%; transform: translateX(-50%); z-index: 70;
    max-width: 560px; padding: 10px 14px; border-radius: 8px;
    background: var(--bg-raised); box-shadow: var(--shadow); border-left: 3px solid var(--danger);
  }
</style>
