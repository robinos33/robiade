<script lang="ts">
  import { onMount } from 'svelte';
  import { app, type View } from './lib/state.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import NewAgent from './components/NewAgent.svelte';
  import Palette from './components/Palette.svelte';
  import ResumePicker from './components/ResumePicker.svelte';
  import QuitConfirm from './components/QuitConfirm.svelte';
  import Inbox from './views/Inbox.svelte';
  import Agents from './views/Agents.svelte';
  import History from './views/History.svelte';
  import MergeRequests from './views/MergeRequests.svelte';
  import Tasks from './views/Tasks.svelte';
  import Stats from './views/Stats.svelte';
  import Environments from './views/Environments.svelte';
  import Databases from './views/Databases.svelte';
  import Code from './views/Code.svelte';
  import ReadingPanel from './components/ReadingPanel.svelte';

  const KEYS: Record<string, View> = { '1': 'inbox', '2': 'agents', '3': 'history', '4': 'tasks', '5': 'stats', '6': 'mr', '7': 'env', '8': 'code' };

  onMount(() => {
    app.init().catch((e) => app.notify(`Démarrage incomplet : ${e}`));
  });

  function onkeydown(e: KeyboardEvent) {
    if (!e.metaKey) return;
    const view = KEYS[e.key];
    if (view) app.view = view;
    else if (e.key === 'k') app.paletteOpen = !app.paletteOpen;
    else if (e.key === 'N') app.newAgentOpen = true; // Shift met la touche en capitale
    else if (e.key === 'n' || e.key === 't') app.newAgent(); // ⌘T, comme un onglet de terminal
    else if (e.key === 'o') app.resumeOpen = !app.resumeOpen;
    else if (e.key === 'p') app.findFile();
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
    {:else if app.view === 'env'}<Environments />
    {:else if app.view === 'db'}<Databases />
    {:else if app.view === 'code'}<Code />
    {:else}<Stats />{/if}
  </main>
  {#if app.reading}<ReadingPanel target={app.reading} />{/if}
</div>

{#if app.newAgentOpen}<NewAgent />{/if}
{#if app.paletteOpen}<Palette />{/if}
{#if app.resumeOpen}<ResumePicker />{/if}
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
