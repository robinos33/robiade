<script module lang="ts">
  import { app } from '../lib/state.svelte';

  export async function copySource(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      app.notify('Source copiée');
    } catch (e) {
      app.notify(`Copie impossible : ${e}`);
    }
  }
</script>

<script lang="ts">
  import type { AssetContent } from '../lib/types';
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

  /**
   * Aperçu d'un fichier déjà lu : image, HTML ou Markdown rendus, sinon texte. Le chargement et la
   * barre d'outils restent à l'appelant (modale des assets, onglet Code).
   */
  let { name, content, error = null, source = false }: {
    name: string;
    /** `null` tant que le fichier se charge. */
    content: AssetContent | null;
    error?: string | null;
    /** HTML et Markdown se lisent rendus ; leur source reste consultable. */
    source?: boolean;
  } = $props();

  const rendered = $derived.by(() => {
    if (!content?.text || source) return null;
    if (content.kind === 'html') return content.text;
    if (content.kind === 'markdown') return MARKDOWN_STYLE + marked.parse(content.text, { async: false, gfm: true });
    return null;
  });
</script>

<div class="body">
  {#if error}
    <p class="muted center">{error}</p>
  {:else if !content}
    <p class="muted center">Chargement…</p>
  {:else if content.dataUrl}
    <div class="image"><img src={content.dataUrl} alt={name} /></div>
  {:else if rendered !== null}
    <!-- Sans `allow-scripts` ni `allow-same-origin` : la page s'affiche, rien ne s'y exécute.
         Le HTML brut d'un Markdown y est donc inoffensif, sans nettoyage à maintenir. -->
    <iframe title={name} sandbox="" srcdoc={rendered} class:md={content.kind === 'markdown'}></iframe>
  {:else}
    <pre class="selectable">{content.text}</pre>
  {/if}
</div>

<style>
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
