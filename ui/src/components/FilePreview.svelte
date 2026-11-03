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
  import { markdownPage } from '../lib/markdown';
  import { colorScheme } from '../lib/theme.svelte';
  import CodeView from './CodeView.svelte';

  /**
   * Aperçu d'un fichier déjà lu : image, HTML ou Markdown rendus, sinon texte coloré d'après
   * `name`. Le chargement et la barre d'outils restent à l'appelant (modale des assets, onglet Code).
   */
  let { name, content, error = null, source = false, line }: {
    name: string;
    /** `null` tant que le fichier se charge. */
    content: AssetContent | null;
    error?: string | null;
    /** HTML et Markdown se lisent rendus ; leur source reste consultable. */
    source?: boolean;
    /** Ligne de texte à montrer, à partir de 1 ; sans effet sur un aperçu rendu. */
    line?: number;
  } = $props();

  const rendered = $derived.by(() => {
    if (!content?.text || source) return null;
    if (content.kind === 'html') return content.text;
    if (content.kind === 'markdown') return markdownPage(content.text, colorScheme());
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
    <CodeView text={content.text ?? ''} {name} {line} />
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
</style>
