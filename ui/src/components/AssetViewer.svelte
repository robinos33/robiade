<script lang="ts">
  import { untrack } from 'svelte';
  import { shortPath } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import type { Asset, AssetContent } from '../lib/types';
  import FilePreview, { copySource } from './FilePreview.svelte';
  import Icon from './Icon.svelte';

  let { name, path, load, assets, onnavigate, onclose }: {
    name: string;
    /** Chemin absolu du fichier ; en changer recharge l'aperçu. */
    path: string;
    /** Lit le fichier ; un rejet porte le message à afficher. */
    load: (path: string) => Promise<AssetContent>;
    /** Assets de l'agent, dans l'ordre de la liste : ← et → passent de l'un à l'autre. */
    assets: Asset[];
    onnavigate: (path: string) => void;
    onclose: () => void;
  } = $props();

  const index = $derived(assets.findIndex((a) => a.path === path));
  const prev = $derived(index > 0 ? assets[index - 1] : undefined);
  const next = $derived(index >= 0 && index < assets.length - 1 ? assets[index + 1] : undefined);

  let content = $state<AssetContent | null>(null);
  let error = $state<string | null>(null);
  /** HTML et Markdown se lisent rendus ; leur source reste consultable. */
  let source = $state(false);
  let close: HTMLButtonElement | undefined = $state();

  /** Les agents sont republiés à chaque événement, `load` avec eux : seul un changement de chemin recharge. */
  $effect(() => {
    const current = path;
    content = null;
    error = null;
    source = false;
    untrack(() => load)(current).then(
      (c) => path === current && (content = c),
      (e) => (error = String(e)),
    );
  });

  $effect(() => close?.focus());

  function onkeydown(e: KeyboardEvent) {
    if (e.metaKey || e.altKey || e.ctrlKey) return;
    if (e.key === 'Escape') onclose();
    else if (e.key === 'ArrowLeft' && prev) onnavigate(prev.path);
    else if (e.key === 'ArrowRight' && next) onnavigate(next.path);
    else return;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="asset-title">
    <header>
      <h2 id="asset-title" class="ellipsis">{name}</h2>
      <span class="mono muted ellipsis path selectable">{shortPath(path, app.env?.home ?? '')}</span>
      {#if assets.length > 1}
        <div class="nav">
          <button class="btn ghost icon" disabled={!prev} onclick={() => prev && onnavigate(prev.path)} title="Précédent (←)"><Icon name="prev" size={14} /></button>
          <span class="muted num">{index + 1} / {assets.length}</span>
          <button class="btn ghost icon" disabled={!next} onclick={() => next && onnavigate(next.path)} title="Suivant (→)"><Icon name="next" size={14} /></button>
        </div>
      {/if}
      {#if content?.kind === 'html' || content?.kind === 'markdown'}
        <button class="btn ghost" onclick={() => (source = !source)}>{source ? 'Rendu' : 'Source'}</button>
      {/if}
      {#if content?.text}
        {@const text = content.text}
        <button class="btn ghost" onclick={() => void copySource(text)}>Copier la source</button>
      {/if}
      <button class="btn ghost icon" bind:this={close} onclick={onclose} title="Fermer (Échap)"><Icon name="close" size={14} /></button>
    </header>
    <FilePreview {name} {content} {error} {source} />
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 70; display: grid; place-items: center; padding: 4vh 4vw;
    background: rgb(0 0 0 / 0.45); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(1100px, 100%); height: 100%; display: flex; flex-direction: column; min-height: 0;
    background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow); overflow: hidden;
  }
  header { display: flex; align-items: center; gap: 12px; padding: 10px 12px 10px 18px; border-bottom: 1px solid var(--line); }
  h2 { margin: 0; font-size: 14px; flex: none; max-width: 40%; }
  .path { flex: 1; font-size: 12px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .icon { padding: 0 6px; }
  .nav { display: flex; align-items: center; gap: 2px; flex: none; font-size: 12px; }
</style>
