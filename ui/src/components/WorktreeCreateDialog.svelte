<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { WorktreeCreation } from '../lib/types';

  let { repository, onclose }: { repository: string; onclose: () => void } = $props();

  let branch = $state('');
  let base = $state('');
  let creation = $state<WorktreeCreation | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let input: HTMLInputElement | undefined = $state();

  $effect(() => input?.focus());

  // Dossier et refus relus côté Rust à chaque saisie : la cible affichée est celle qui sera créée.
  $effect(() => {
    const wanted = branch.trim();
    const timer = setTimeout(() => {
      api.prepareWorktreeCreation(repository, wanted).then(
        (c) => {
          if (wanted !== branch.trim()) return;
          creation = c;
          if (!c.bases.includes(base)) base = c.bases[0] ?? '';
          error = null;
        },
        (e) => (error = String(e)),
      );
    }, 150);
    return () => clearTimeout(timer);
  });

  const ready = $derived(creation !== null && branch.trim() !== '' && creation.blocker === null && base !== '');

  async function create() {
    if (!creation || !ready) return;
    busy = true;
    error = null;
    const dir = creation.dir;
    try {
      const fetched = await api.createWorktree(repository, branch.trim(), base);
      if (!fetched) app.notify(`${base} n'a pas pu être mis à jour depuis origin : la base locale a servi.`);
      await app.refreshWorktrees();
      const created = app.worktrees.find((w) => w.path === dir);
      if (created) app.selectWorktree(created);
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
    } else if (e.key === 'Enter' && !busy) {
      e.preventDefault();
      void create();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation">
  <div class="dialog" role="dialog" aria-labelledby="wt-create-title">
    <h2 id="wt-create-title">Nouveau worktree</h2>
    <div class="grid">
      <label for="wt-branch">Branche</label>
      <input
        id="wt-branch"
        class="field mono"
        bind:this={input}
        bind:value={branch}
        maxlength="100"
        spellcheck="false"
        placeholder="ticket-1234-titre"
      />
      <label for="wt-base">Depuis</label>
      <select id="wt-base" class="field mono" bind:value={base} disabled={!creation?.bases.length}>
        {#each creation?.bases ?? [] as b (b)}<option value={b}>{b}</option>{/each}
      </select>
      <span class="k">Dépôt</span>
      <span class="mono path">{repository}</span>
      <span class="k">Worktree</span>
      <span class="mono path">{branch.trim() && creation ? creation.dir : '—'}</span>
    </div>
    {#if branch.trim() && creation?.blocker}
      <p class="error">{creation.blocker}</p>
    {:else}
      <p class="muted note">
        Une branche neuve, créée depuis {base || 'la base'} mise à jour depuis origin ; aucun hook git ne tourne.
      </p>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    <footer>
      <button class="btn" onclick={onclose}>Annuler</button>
      <button class="btn primary" disabled={!ready || busy} onclick={() => void create()}>
        {busy ? 'Création du worktree…' : 'Créer le worktree'}
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
  h2 { margin: 0; font-size: 15px; }
  .grid { display: grid; grid-template-columns: 70px 1fr; gap: 8px 12px; align-items: center; }
  label, .k { color: var(--ink-3); font-size: 12.5px; }
  .field { height: 30px; }
  .path { font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; }
  .note { margin: 0; font-size: 12px; }
  .error { margin: 0; color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
</style>
