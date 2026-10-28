<script lang="ts">
  import type { DbStage } from '../lib/types';
  import StageBadge from './StageBadge.svelte';

  let {
    statement,
    connection,
    stage,
    target,
    onconfirm,
    onclose,
  }: {
    statement: string;
    connection: string;
    stage: DbStage;
    target: string;
    onconfirm: () => void;
    onclose: () => void;
  } = $props();

  let cancel: HTMLButtonElement | undefined = $state();
  // Annuler a le focus : un Entrée réflexe ne lance pas l'écriture.
  $effect(() => cancel?.focus());

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation">
  <div class="dialog" role="alertdialog" aria-labelledby="st-title">
    <h2 id="st-title">Exécuter cette écriture ?</h2>
    <div class="grid">
      <span class="k">Connexion</span>
      <span class="path"><StageBadge {stage} /> {connection}</span>
      <span class="k">Cible</span>
      <span class="mono path">{target}</span>
    </div>
    <!-- L'instruction exacte qui partira, rendue en texte. -->
    <pre class="mono statement">{statement}</pre>
    <p class="muted note">Elle part seule et est validée aussitôt : pas d'annulation après coup.</p>
    <footer>
      <button class="btn" bind:this={cancel} onclick={onclose}>Annuler</button>
      <button class="btn danger" onclick={onconfirm}>Exécuter</button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 14vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(640px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 12px;
  }
  h2 { margin: 0; font-size: 15px; }
  .grid { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; align-items: baseline; }
  .k { color: var(--ink-3); font-size: 12.5px; }
  .path { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; }
  .statement {
    margin: 0; max-height: 40vh; overflow: auto; padding: 10px 12px; border-radius: var(--radius-sm);
    background: var(--bg-sunken); border: 1px solid var(--line); white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .note { margin: 0; font-size: 12px; }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .danger { background: var(--danger); border-color: transparent; color: #fff; font-weight: 550; }
  .danger:hover { filter: brightness(1.08); }
</style>
