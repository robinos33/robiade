<script lang="ts">
  import { api } from '../lib/api';
  import { shortPath } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import type { Asset, AssetContent } from '../lib/types';
  import Icon from './Icon.svelte';
  import { marked } from 'marked';

  /** Feuille de style du Markdown rendu : l'iframe n'hérite pas de celle de Robiade. */
  const MARKDOWN_STYLE = `<style>
    :root { color-scheme: light dark; }
    body { font: 14px/1.6 -apple-system, BlinkMacSystemFont, sans-serif; max-width: 78ch; margin: 24px auto; padding: 0 24px; }
    h1, h2, h3 { line-height: 1.25; margin: 1.4em 0 0.5em; }
    h1 { font-size: 1.6em; } h2 { font-size: 1.3em; } h3 { font-size: 1.1em; }
    code { font: 12.5px ui-monospace, SFMono-Regular, Menlo, monospace; background: rgb(127 127 127 / 0.15); padding: 1px 4px; border-radius: 4px; }
    pre { background: rgb(127 127 127 / 0.12); padding: 12px 14px; border-radius: 8px; overflow-x: auto; }
    pre code { background: none; padding: 0; }
    table { border-collapse: collapse; } th, td { border: 1px solid rgb(127 127 127 / 0.35); padding: 4px 10px; text-align: left; }
    blockquote { margin: 0; padding-left: 14px; border-left: 3px solid rgb(127 127 127 / 0.4); opacity: 0.85; }
    img { max-width: 100%; }
  </style>`;

  let { agentId, asset, assets, onnavigate, onclose }: {
    agentId: string;
    asset: Asset;
    /** Assets de l'agent, dans l'ordre de la liste : ← et → passent de l'un à l'autre. */
    assets: Asset[];
    onnavigate: (path: string) => void;
    onclose: () => void;
  } = $props();

  const index = $derived(assets.findIndex((a) => a.path === asset.path));
  const prev = $derived(index > 0 ? assets[index - 1] : undefined);
  const next = $derived(index >= 0 && index < assets.length - 1 ? assets[index + 1] : undefined);

  let content = $state<AssetContent | null>(null);
  let error = $state<string | null>(null);
  /** HTML et Markdown se lisent rendus ; leur source reste consultable. */
  let source = $state(false);
  let close: HTMLButtonElement | undefined = $state();

  /** Les agents sont republiés à chaque événement, `asset` avec eux : seul un changement de chemin recharge. */
  const path = $derived(asset.path);

  $effect(() => {
    const current = path;
    content = null;
    error = null;
    source = false;
    api.readAsset(agentId, current).then(
      (c) => path === current && (content = c),
      (e) => (error = String(e)),
    );
  });

  $effect(() => close?.focus());

  const rendered = $derived.by(() => {
    if (!content?.text || source) return null;
    if (content.kind === 'html') return content.text;
    if (content.kind === 'markdown') return MARKDOWN_STYLE + marked.parse(content.text, { async: false, gfm: true });
    return null;
  });

  async function copySource(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      app.notify('Source copiée');
    } catch (e) {
      app.notify(`Copie impossible : ${e}`);
    }
  }

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
      <h2 id="asset-title" class="ellipsis">{asset.name}</h2>
      <span class="mono muted ellipsis path selectable">{shortPath(asset.path, app.env?.home ?? '')}</span>
      {#if assets.length > 1}
        <div class="nav">
          <button class="btn ghost icon" disabled={!prev} onclick={() => prev && onnavigate(prev.path)} title="Précédent (←)"><Icon name="prev" size={14} /></button>
          <span class="muted num">{index + 1} / {assets.length}</span>
          <button class="btn ghost icon" disabled={!next} onclick={() => next && onnavigate(next.path)} title="Suivant (→)"><Icon name="next" size={14} /></button>
        </div>
      {/if}
      {#if asset.kind === 'html' || asset.kind === 'markdown'}
        <button class="btn ghost" onclick={() => (source = !source)}>{source ? 'Rendu' : 'Source'}</button>
      {/if}
      {#if content?.text}
        {@const text = content.text}
        <button class="btn ghost" onclick={() => void copySource(text)}>Copier la source</button>
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
      {:else if rendered !== null}
        <!-- Sans `allow-scripts` ni `allow-same-origin` : la page s'affiche, rien ne s'y exécute.
             Le HTML brut d'un Markdown y est donc inoffensif, sans nettoyage à maintenir. -->
        <iframe title={asset.name} sandbox="" srcdoc={rendered} class:md={content.kind === 'markdown'}></iframe>
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
  .nav { display: flex; align-items: center; gap: 2px; flex: none; font-size: 12px; }
  .body { flex: 1; min-height: 0; display: flex; background: var(--bg); }
  .center { margin: auto; }
  .image {
    flex: 1; display: grid; place-items: center; overflow: auto; padding: 16px;
    background: repeating-conic-gradient(var(--bg-sunken) 0 25%, var(--bg) 0 50%) 0 0 / 16px 16px;
  }
  img { max-width: 100%; max-height: 100%; object-fit: contain; }
  iframe { flex: 1; border: 0; background: #fff; }
  iframe.md { background: var(--bg); }
  pre {
    flex: 1; margin: 0; padding: 16px 20px; overflow: auto; font: 12.5px/1.55 var(--mono);
    white-space: pre-wrap; word-break: break-word;
  }
</style>
