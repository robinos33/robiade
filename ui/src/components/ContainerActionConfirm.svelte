<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { Container, ContainerAction, ContainerActionPreview } from '../lib/types';

  let { container, action, onclose, ondone }: {
    container: Container;
    action: ContainerAction;
    onclose: () => void;
    ondone: () => void;
  } = $props();

  const VERB: Record<ContainerAction, { title: string; button: string; busy: string; done: string }> = {
    start: { title: 'Démarrer', button: 'Démarrer', busy: 'Démarrage…', done: 'démarré' },
    stop: { title: 'Arrêter', button: 'Arrêter', busy: 'Arrêt…', done: 'arrêté' },
    restart: { title: 'Redémarrer', button: 'Redémarrer', busy: 'Redémarrage…', done: 'redémarré' },
  };
  const verb = $derived(VERB[action]);

  let preview = $state<ContainerActionPreview | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let cancel: HTMLButtonElement | undefined = $state();

  $effect(() => cancel?.focus());

  // La cible est relue sur Docker : l'état affiché dans la liste peut dater.
  $effect(() => {
    api.prepareContainerAction(container.id, action).then(
      (p) => (preview = p),
      (e) => (error = String(e)),
    );
  });

  async function confirm() {
    busy = true;
    error = null;
    try {
      await api.runContainerAction(container.id, action);
      app.notify(`Conteneur ${container.name} ${verb.done}.`);
      ondone();
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation">
  <div class="dialog" role="alertdialog" aria-labelledby="ct-title">
    <h2 id="ct-title">{verb.title} le conteneur {container.name} ?</h2>
    {#if !preview}
      <p class="muted">{error ? '' : 'Lecture du conteneur…'}</p>
    {:else}
      {@const c = preview.container}
      <div class="grid">
        <span class="k">Conteneur</span>
        <span class="mono path">{c.name} · {c.id.slice(0, 12)}</span>
        <span class="k">Image</span>
        <span class="mono path">{c.image}</span>
        <span class="k">Compose</span>
        <span class="mono path">{c.composeProject ? `${c.composeProject} / ${c.service ?? '?'}` : 'hors Compose'}</span>
        {#if c.workingDir}
          <span class="k">Dossier</span>
          <span class="mono path">{c.workingDir}</span>
        {/if}
        <span class="k">État</span>
        <span class="path">{c.status}</span>
      </div>
      {#if preview.blocker}<p class="error">{preview.blocker}</p>{/if}
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    <footer>
      <button class="btn" bind:this={cancel} onclick={onclose}>Annuler</button>
      <button
        class="btn"
        class:danger={action !== 'start'}
        disabled={!preview || !!preview.blocker || busy}
        onclick={() => void confirm()}
      >
        {busy ? verb.busy : verb.button}
      </button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 18vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(560px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 12px;
  }
  h2 { margin: 0; font-size: 15px; overflow-wrap: anywhere; }
  .grid { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; align-items: baseline; }
  .k { color: var(--ink-3); font-size: 12.5px; }
  .path { font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; }
  .error { margin: 0; color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .danger { background: var(--danger); border-color: transparent; color: #fff; font-weight: 550; }
  .danger:hover:not(:disabled) { filter: brightness(1.08); }
  .btn:disabled { opacity: 0.5; }
</style>
