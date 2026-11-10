<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import { shortPath } from '../lib/format';
  import type { Routine, RoutineAction, RoutineActionPreview } from '../lib/types';

  let { routine, action, onclose, ondone }: {
    routine: Routine;
    action: RoutineAction;
    onclose: () => void;
    ondone: () => void;
  } = $props();

  const VERB: Record<RoutineAction, { title: string; button: string; busy: string; done: string; effect: string }> = {
    run: {
      title: 'Lancer maintenant',
      button: 'Lancer',
      busy: 'Lancement…',
      done: 'lancée',
      effect: 'launchd lance la commande tout de suite, comme à un créneau : ses effets (messages, écritures) partent pour de vrai.',
    },
    disable: {
      title: 'Désactiver',
      button: 'Désactiver',
      busy: 'Désactivation…',
      done: 'désactivée',
      effect: 'launchd ne la lance plus, et ne la recharge pas à la prochaine session. Le plist reste.',
    },
    enable: {
      title: 'Activer',
      button: 'Activer',
      busy: 'Activation…',
      done: 'activée',
      effect: 'launchd la recharge : elle repart à ses créneaux.',
    },
    delete: {
      title: 'Supprimer',
      button: 'Supprimer',
      busy: 'Suppression…',
      done: 'supprimée',
      effect: 'launchd la décharge et le plist est supprimé. Ses journaux restent.',
    },
  };
  const verb = $derived(VERB[action]);
  const home = $derived(app.env?.home ?? '');

  let preview = $state<RoutineActionPreview | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let cancel: HTMLButtonElement | undefined = $state();

  $effect(() => cancel?.focus());

  // La routine est relue sur launchd : l'état affiché dans la liste peut dater.
  $effect(() => {
    api.prepareRoutineAction(routine.label, action).then(
      (p) => (preview = p),
      (e) => (error = String(e)),
    );
  });

  async function confirm() {
    busy = true;
    error = null;
    try {
      await api.runRoutineAction(routine.label, action);
      app.notify(`Routine ${routine.name} ${verb.done}.`);
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
  <div class="dialog" role="alertdialog" aria-labelledby="rt-title">
    <h2 id="rt-title">{verb.title} la routine {routine.name} ?</h2>
    <p class="muted">{verb.effect}</p>
    {#if !preview}
      <p class="muted">{error ? '' : 'Lecture de la routine…'}</p>
    {:else}
      {@const r = preview.routine}
      <div class="grid">
        <span class="k">Label</span>
        <span class="mono path">{r.label}</span>
        <span class="k">Commande</span>
        <span class="mono path">{r.command.join(' ') || '—'}</span>
        <span class="k">Planning</span>
        <span class="path">{r.schedule ?? '—'}</span>
        <span class="k">Plist</span>
        <span class="mono path">{shortPath(r.file, home)}</span>
      </div>
      {#if preview.blocker}<p class="error">{preview.blocker}</p>{/if}
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    <footer>
      <button class="btn" bind:this={cancel} onclick={onclose}>Annuler</button>
      <button
        class="btn"
        class:danger={action === 'delete' || action === 'disable'}
        class:primary={action === 'run' || action === 'enable'}
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
  p { margin: 0; }
  .grid { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; align-items: baseline; }
  .k { color: var(--ink-3); font-size: 12.5px; }
  .path { font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; }
  .error { color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .danger { background: var(--danger); border-color: transparent; color: #fff; font-weight: 550; }
  .danger:hover:not(:disabled) { filter: brightness(1.08); }
  .btn:disabled { opacity: 0.5; }
</style>
