<script lang="ts">
  import { onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import { app, type View } from './lib/state.svelte';
  import { sidebar } from './lib/sidebar.svelte';
  import Logo from './components/Logo.svelte';
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
  import Settings from './views/Settings.svelte';
  import ReadingPanel from './components/ReadingPanel.svelte';

  const KEYS: Record<string, View> = { '1': 'inbox', '2': 'agents', '3': 'history', '4': 'tasks', '5': 'stats', '6': 'mr', '7': 'env', '8': 'code' };

  /**
   * Écran de démarrage, posé sur l'interface qui se remplit dessous. Il reste le temps de l'`init`,
   * au moins un tour du point (900 ms) pour ne pas flasher, et jamais plus de 5 s, quoi qu'il arrive.
   */
  let booting = $state(true);

  onMount(() => {
    const ready = app.init().catch((e) => app.notify(`Démarrage incomplet : ${e}`));
    const turn = new Promise((r) => setTimeout(r, 900));
    const ceiling = new Promise((r) => setTimeout(r, 5000));
    void Promise.race([Promise.all([ready, turn]), ceiling]).then(() => (booting = false));
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
    else if (e.key === ',') app.view = 'settings'; // ⌘, : les réglages, comme toute app macOS
    else return;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<div class="shell" class:sidebar-collapsed={sidebar.collapsed}>
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
    {:else if app.view === 'settings'}<Settings />
    {:else}<Stats />{/if}
  </main>
  {#if app.reading}<ReadingPanel target={app.reading} />{/if}
</div>

{#if app.newAgentOpen}<NewAgent />{/if}
{#if app.paletteOpen}<Palette />{/if}
{#if app.resumeOpen}<ResumePicker />{/if}
{#if app.quitRequest}<QuitConfirm />{/if}
{#if booting}
  <div class="splash" out:fade={{ duration: 200 }} data-tauri-drag-region><Logo size={96} orbit="loop" /></div>
{/if}
{#if app.toast}<div class="toast" role="status">{app.toast}</div>{/if}

<style>
  /* Sous le toast, pour qu'un « Démarrage incomplet » se lise tout de suite, et sous les dialogues (80). */
  .splash {
    position: fixed; inset: 0; z-index: 65; display: grid; place-items: center;
    background: var(--bg); --logo-halo: var(--bg);
  }
  .shell { display: flex; height: 100%; }
  main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .toast {
    position: fixed; bottom: 18px; left: 50%; transform: translateX(-50%); z-index: 70;
    max-width: 560px; padding: 10px 14px; border-radius: 8px;
    background: var(--bg-raised); box-shadow: var(--shadow); border-left: 3px solid var(--danger);
  }
</style>
