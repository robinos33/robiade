<script lang="ts">
  import { findFiles, type Match } from '../lib/fuzzy';
  import { baseName } from '../lib/fileTree';
  import Icon from './Icon.svelte';

  /** Aller à un fichier par son nom (⌘P). Sans saisie : les derniers fichiers ouverts. */
  let { files, secrets, recent, onpick, onclose }: {
    files: readonly string[];
    secrets: ReadonlySet<string>;
    recent: readonly string[];
    onpick: (path: string) => void;
    onclose: () => void;
  } = $props();

  let query = $state('');
  let index = $state(0);
  let input: HTMLInputElement | undefined = $state();

  const matches = $derived<Match[]>(
    query.trim() ? findFiles(files, query) : recent.map((path) => ({ path, positions: [], score: 0 })),
  );

  $effect(() => input?.focus());

  /** Nom et dossier découpés en morceaux surlignés ou non : du texte, jamais du HTML. */
  function pieces(text: string, offset: number, positions: number[]) {
    const hit = new Set(positions.map((p) => p - offset));
    const out: { text: string; hit: boolean }[] = [];
    for (let i = 0; i < text.length; i++) {
      const last = out.at(-1);
      if (last && last.hit === hit.has(i)) last.text += text[i];
      else out.push({ text: text[i]!, hit: hit.has(i) });
    }
    return out;
  }

  function pick(m: Match | undefined) {
    if (!m) return;
    onclose();
    onpick(m.path);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') index = Math.min(index + 1, matches.length - 1);
    else if (e.key === 'ArrowUp') index = Math.max(index - 1, 0);
    else if (e.key === 'Enter') pick(matches[index]);
    else if (e.key === 'Escape') onclose();
    else return;
    e.preventDefault();
  }
</script>

<div class="scrim" role="presentation" onclick={onclose}>
  <div class="dialog" role="dialog" aria-label="Aller au fichier" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
    <input placeholder="Nom de fichier, ou morceau de chemin…" bind:value={query} bind:this={input} {onkeydown} oninput={() => (index = 0)} />
    <ul>
      {#each matches as m, i (m.path)}
        {@const name = baseName(m.path)}
        {@const dir = m.path.slice(0, m.path.length - name.length)}
        <li>
          <button class:active={i === index} onmouseenter={() => (index = i)} onclick={() => pick(m)}>
            <Icon name={secrets.has(m.path) ? 'lock' : 'file'} size={14} />
            <span class="name">{#each pieces(name, dir.length, m.positions) as p, k (k)}{#if p.hit}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}</span>
            <span class="dir">{#each pieces(dir, 0, m.positions) as p, k (k)}{#if p.hit}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}</span>
          </button>
        </li>
      {:else}
        <li class="none">{query.trim() ? 'Aucun fichier ne correspond.' : 'Tape un nom de fichier.'}</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 60; display: grid; justify-items: center; align-items: start; padding-top: 12vh; background: rgb(0 0 0 / 0.3); }
  .dialog { width: min(680px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow); overflow: hidden; outline: none; }
  input { width: 100%; height: 48px; padding: 0 16px; border: 0; border-bottom: 1px solid var(--line); background: none; font-size: 15px; outline: none; }
  ul { list-style: none; margin: 0; padding: 6px; max-height: 420px; overflow-y: auto; }
  button { display: flex; align-items: center; gap: 10px; width: 100%; height: 32px; padding: 0 10px; border: 0; border-radius: 6px; background: none; text-align: left; color: var(--ink-2); }
  button.active { background: var(--bg-active); color: var(--ink); }
  .name { flex: none; max-width: 50%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink); }
  .dir { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink-3); font-size: 12px; }
  mark { background: none; color: var(--accent); font-weight: 650; }
  .none { padding: 10px; color: var(--ink-3); }
</style>
