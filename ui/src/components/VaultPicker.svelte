<script lang="ts">
  import { api } from '../lib/api';
  import type { DbSource } from '../lib/types';

  let { onpick }: { onpick: (source: Extract<DbSource, { kind: 'vault' }>) => void } = $props();

  /** Montages déjà saisis : une commodité locale, jamais une donnée de référence. */
  const MOUNTS_KEY = 'robiade.vault.mounts';
  function remembered(): string[] {
    try {
      return JSON.parse(localStorage.getItem(MOUNTS_KEY) ?? '[]');
    } catch {
      return [];
    }
  }
  function remember(mount: string) {
    try {
      localStorage.setItem(MOUNTS_KEY, JSON.stringify([mount, ...remembered().filter((m) => m !== mount)].slice(0, 10)));
    } catch {
      // Stockage indisponible : on resaisira le montage.
    }
  }

  const mounts = remembered();
  let mount = $state(mounts[0] ?? '');
  let prefix = $state('internal/');
  let paths = $state<string[] | null>(null);
  let filter = $state('');
  let path = $state<string | null>(null);
  let keys = $state<string[] | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);

  const shown = $derived.by(() => {
    const words = filter.trim().toLowerCase().split(/\s+/).filter(Boolean);
    return (paths ?? []).filter((p) => words.every((w) => p.toLowerCase().includes(w)));
  });

  async function list() {
    if (!mount.trim() || busy) return;
    busy = true;
    error = null;
    path = null;
    keys = null;
    try {
      paths = await api.vaultPaths(mount.trim(), prefix.trim());
      remember(mount.trim());
    } catch (e) {
      error = String(e);
      paths = null;
    } finally {
      busy = false;
    }
  }

  async function open(p: string) {
    path = p;
    keys = null;
    error = null;
    try {
      keys = await api.vaultKeys(mount.trim(), p);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="picker">
  <div class="bar">
    <input class="field mono" list="vault-mounts" bind:value={mount} placeholder="montage, ex. secret" spellcheck="false" />
    <datalist id="vault-mounts">{#each mounts as m (m)}<option value={m}></option>{/each}</datalist>
    <input
      class="field mono"
      bind:value={prefix}
      placeholder="préfixe, ex. internal/prod/"
      spellcheck="false"
      onkeydown={(e) => e.key === 'Enter' && (e.preventDefault(), void list())}
    />
    <button class="btn" onclick={() => void list()} disabled={busy || !mount.trim()}>{busy ? 'Lecture…' : 'Lister'}</button>
  </div>

  {#if error}<p class="error">{error}</p>{/if}

  {#if paths}
    <input class="field" bind:value={filter} placeholder="Filtrer {paths.length} secret{paths.length > 1 ? 's' : ''}…" />
    <div class="cols">
      <ul class="list">
        {#each shown as p (p)}
          <li><button class="mono" class:active={p === path} onclick={() => void open(p)}>{p}</button></li>
        {:else}
          <li class="muted empty">Aucun secret.</li>
        {/each}
      </ul>
      <ul class="list keys">
        {#if path && keys}
          {#each keys as k (k)}
            <li>
              <button class="mono" onclick={() => onpick({ kind: 'vault', mount: mount.trim(), path: path!, key: k })}>{k}</button>
            </li>
          {:else}
            <li class="muted empty">Secret vide.</li>
          {/each}
        {:else}
          <li class="muted empty">{path ? 'Lecture des clés…' : 'Choisir un secret'}</li>
        {/if}
      </ul>
    </div>
  {/if}
  <p class="muted note">Seuls les noms s'affichent ici ; les valeurs sont lues au moment de se connecter.</p>
</div>

<style>
  .picker { display: flex; flex-direction: column; gap: 8px; padding: 10px; border: 1px solid var(--line); border-radius: var(--radius); }
  .bar { display: grid; grid-template-columns: 1fr 1fr auto; gap: 6px; }
  .field { height: 28px; }
  .cols { display: grid; grid-template-columns: 2fr 1fr; gap: 6px; }
  .list { list-style: none; margin: 0; padding: 0; max-height: 200px; overflow-y: auto; border: 1px solid var(--line); border-radius: var(--radius-sm); }
  .list button {
    width: 100%; text-align: left; border: 0; background: none; padding: 3px 8px; font-size: 12px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .list button:hover, .list button.active { background: var(--bg-hover); }
  .empty { padding: 6px 8px; font-size: 12px; }
  .note { margin: 0; font-size: 11.5px; }
  .error { margin: 0; color: var(--danger); font-size: 12.5px; }
</style>
