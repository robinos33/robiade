<script lang="ts">
  import { api } from '../lib/api';
  import type { FileChange } from '../lib/types';
  import Icon from './Icon.svelte';

  let { agentId, change, onclose }: { agentId: string; change: FileChange; onclose: () => void } = $props();

  let diff = $state<string | null>(null);
  let error = $state<string | null>(null);
  let close: HTMLButtonElement | undefined = $state();

  $effect(() => {
    const path = change.path;
    diff = null;
    error = null;
    api.workingDiff(agentId, path).then(
      (d) => change.path === path && (diff = d),
      (e) => (error = String(e)),
    );
  });

  $effect(() => close?.focus());

  /** Classe d'une ligne de diff unifié, d'après son premier caractère : le texte reste du texte. */
  function kind(line: string): string {
    if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('diff ') || line.startsWith('index ')) return 'meta';
    if (line.startsWith('@@')) return 'hunk';
    if (line.startsWith('+')) return 'add';
    if (line.startsWith('-')) return 'del';
    return '';
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    onclose();
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="diff-title">
    <header>
      <h2 id="diff-title" class="mono ellipsis">{change.path}</h2>
      {#if change.added !== null}<span class="num add">+{change.added}</span>{/if}
      {#if change.removed !== null}<span class="num del">−{change.removed}</span>{/if}
      <span class="spacer"></span>
      <button class="btn ghost icon" bind:this={close} onclick={onclose} title="Fermer (Échap)"><Icon name="close" size={14} /></button>
    </header>
    <div class="body selectable">
      {#if error}
        <p class="muted center">{error}</p>
      {:else if diff === null}
        <p class="muted center">Chargement…</p>
      {:else if diff.trim() === ''}
        <p class="muted center">Aucune différence textuelle (fichier binaire ou changement de mode).</p>
      {:else}
        <pre>{#each diff.split('\n') as line, i (i)}<span class="line {kind(line)}">{line || ' '}</span>{/each}</pre>
      {/if}
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 70; display: grid; place-items: center; padding: 4vh 4vw;
    background: rgb(0 0 0 / 0.45); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(1200px, 100%); height: 100%; display: flex; flex-direction: column; min-height: 0;
    background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow); overflow: hidden;
  }
  header { display: flex; align-items: center; gap: 10px; padding: 10px 12px 10px 18px; border-bottom: 1px solid var(--line); }
  h2 { margin: 0; font-size: 13px; min-width: 0; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .spacer { flex: 1; }
  .icon { padding: 0 6px; }
  .add { color: var(--ok); }
  .del { color: var(--danger); }
  .body { flex: 1; min-height: 0; overflow: auto; background: var(--bg); }
  .center { margin: 40px auto; text-align: center; }
  pre { margin: 0; padding: 12px 0; font: 12px/1.5 var(--mono); }
  .line { display: block; padding: 0 16px; white-space: pre; }
  .line.add { background: rgb(46 158 91 / 0.14); color: inherit; }
  .line.del { background: rgb(220 70 70 / 0.14); color: inherit; }
  .line.hunk { color: var(--info); background: var(--bg-sunken); }
  .line.meta { color: var(--ink-3); font-weight: 600; }
</style>
