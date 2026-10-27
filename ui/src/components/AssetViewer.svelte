<script lang="ts">
  import { api } from '../lib/api';
  import { shortPath } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import type { Asset, AssetContent } from '../lib/types';
  import Icon from './Icon.svelte';

  let { agentId, asset, onclose }: { agentId: string; asset: Asset; onclose: () => void } = $props();

  let content = $state<AssetContent | null>(null);
  let error = $state<string | null>(null);
  /** Une page HTML se lit rendue ; sa source reste consultable. */
  let source = $state(false);
  let close: HTMLButtonElement | undefined = $state();

  $effect(() => {
    const path = asset.path;
    content = null;
    error = null;
    source = false;
    api.readAsset(agentId, path).then(
      (c) => asset.path === path && (content = c),
      (e) => (error = String(e)),
    );
  });

  $effect(() => close?.focus());

  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    onclose();
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="asset-title">
    <header>
      <h2 id="asset-title" class="ellipsis">{asset.name}</h2>
      <span class="mono muted ellipsis path selectable">{shortPath(asset.path, app.env?.home ?? '')}</span>
      {#if asset.kind === 'html'}
        <button class="btn ghost" onclick={() => (source = !source)}>{source ? 'Rendu' : 'Source'}</button>
      {/if}
      <button class="btn ghost icon" bind:this={close} onclick={onclose} title="Fermer (Échap)"><Icon name="close" size={14} /></button>
    </header>
    <div class="body">
      {#if error}
        <p class="muted center">{error}</p>
      {:else if !content}
        <p class="muted center">Chargement…</p>
      {:else if content.dataUrl}
        <div class="image"><img src={content.dataUrl} alt={asset.name} /></div>
      {:else if content.kind === 'html' && !source}
        <!-- Sans `allow-scripts` ni `allow-same-origin` : la page s'affiche, rien ne s'y exécute. -->
        <iframe title={asset.name} sandbox="" srcdoc={content.text ?? ''}></iframe>
      {:else}
        <pre class="selectable">{content.text}</pre>
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
    width: min(1100px, 100%); height: 100%; display: flex; flex-direction: column; min-height: 0;
    background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow); overflow: hidden;
  }
  header { display: flex; align-items: center; gap: 12px; padding: 10px 12px 10px 18px; border-bottom: 1px solid var(--line); }
  h2 { margin: 0; font-size: 14px; flex: none; max-width: 40%; }
  .path { flex: 1; font-size: 12px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .icon { padding: 0 6px; }
  .body { flex: 1; min-height: 0; display: flex; background: var(--bg); }
  .center { margin: auto; }
  .image {
    flex: 1; display: grid; place-items: center; overflow: auto; padding: 16px;
    background: repeating-conic-gradient(var(--bg-sunken) 0 25%, var(--bg) 0 50%) 0 0 / 16px 16px;
  }
  img { max-width: 100%; max-height: 100%; object-fit: contain; }
  iframe { flex: 1; border: 0; background: #fff; }
  pre {
    flex: 1; margin: 0; padding: 16px 20px; overflow: auto; font: 12.5px/1.55 var(--mono);
    white-space: pre-wrap; word-break: break-word;
  }
</style>
