<script lang="ts">
  import { api } from '../lib/api';
  import type { FileChange } from '../lib/types';
  import DiffViewer from './DiffViewer.svelte';
  import Icon from './Icon.svelte';

  /** `activity` change à chaque événement de l'agent : la liste se relit quand il travaille. */
  let { agentId, activity }: { agentId: string; activity: number } = $props();

  let changes = $state<FileChange[] | null>(null);
  let error = $state<string | null>(null);
  let opened = $state<FileChange | null>(null);

  const LETTER: Record<FileChange['kind'], string> = {
    modified: 'M', added: 'A', deleted: 'D', renamed: 'R', untracked: 'U',
  };

  async function load(id: string) {
    try {
      const found = await api.workingChanges(id);
      if (id !== agentId) return;
      changes = found;
      error = null;
    } catch (e) {
      if (id === agentId) error = String(e);
    }
  }

  $effect(() => {
    const id = agentId;
    void activity;
    const timer = setTimeout(() => void load(id), 400);
    return () => clearTimeout(timer);
  });

  const totals = $derived(
    (changes ?? []).reduce((t, c) => ({ added: t.added + (c.added ?? 0), removed: t.removed + (c.removed ?? 0) }), {
      added: 0,
      removed: 0,
    }),
  );

  function split(path: string): [string, string] {
    const i = path.lastIndexOf('/');
    return i < 0 ? ['', path] : [path.slice(0, i + 1), path.slice(i + 1)];
  }
</script>

<div class="head">
  {#if changes?.length}
    <span class="num muted">{changes.length} fichier{changes.length > 1 ? 's' : ''}</span>
    <span class="num add">+{totals.added}</span><span class="num del">−{totals.removed}</span>
  {/if}
  <span class="spacer"></span>
  <button class="btn ghost refresh" onclick={() => void load(agentId)} title="Relire les modifications">
    <Icon name="resume" size={11} /> Relire
  </button>
</div>

{#if error}
  <p class="muted note">{error}</p>
{:else if changes === null}
  <p class="muted note">Lecture…</p>
{:else if changes.length === 0}
  <p class="muted note">Aucune modification en cours depuis le dernier commit.</p>
{:else}
  <ul>
    {#each changes as c (c.path)}
      {@const [dir, name] = split(c.path)}
      <li>
        <button class="file" onclick={() => (opened = c)} title={c.path}>
          <span class="letter {c.kind}">{LETTER[c.kind]}</span>
          <span class="path"><span class="name">{name}</span><span class="dir muted">{dir}</span></span>
          {#if c.added === null}
            <span class="muted num">bin</span>
          {:else}
            <span class="num add">+{c.added}</span><span class="num del">−{c.removed}</span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>
{/if}

{#if opened}
  <DiffViewer {agentId} change={opened} onclose={() => (opened = null)} />
{/if}

<style>
  .head { display: flex; align-items: center; gap: 6px; font-size: 12px; margin-bottom: 6px; }
  .spacer { flex: 1; }
  .refresh { height: 24px; padding: 0 6px; font-size: 11.5px; }
  .note { font-size: 11.5px; }
  ul { list-style: none; margin: 0 -6px; padding: 0; }
  .file {
    display: flex; align-items: center; gap: 6px; width: 100%; height: 26px; padding: 0 6px; font-size: 12px;
    border: 0; border-radius: var(--radius-sm); background: none; color: var(--ink-2); text-align: left;
  }
  .file:hover { background: var(--bg-hover); color: var(--ink); }
  .letter { font: 600 10.5px var(--mono); width: 12px; flex: none; color: var(--ink-3); }
  .letter.added, .letter.untracked { color: var(--ok); }
  .letter.deleted { color: var(--danger); }
  .letter.renamed { color: var(--info); }
  .path { flex: 1; min-width: 0; display: flex; gap: 6px; overflow: hidden; white-space: nowrap; }
  .name { flex: none; max-width: 100%; overflow: hidden; text-overflow: ellipsis; }
  .dir { min-width: 0; overflow: hidden; text-overflow: ellipsis; direction: rtl; text-align: left; }
  .add { color: var(--ok); font-size: 11px; }
  .del { color: var(--danger); font-size: 11px; }
</style>
