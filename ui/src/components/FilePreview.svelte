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

  /**
   * Menu de l'image, à la place de celui de WebKit : son « Copier l'image » laisse le
   * presse-papiers vide dans la fenêtre Tauri.
   */
  let menu = $state<{ img: HTMLImageElement; x: number; y: number } | null>(null);

  function openMenu(e: MouseEvent) {
    e.preventDefault();
    menu = {
      img: e.currentTarget as HTMLImageElement,
      x: Math.min(e.clientX, window.innerWidth - 180),
      y: Math.min(e.clientY, window.innerHeight - 48),
    };
  }

  /** Échap ferme le menu sans fermer la modale qui contient l'aperçu. */
  function closeMenu(e: KeyboardEvent | MouseEvent) {
    if (!menu || (e instanceof KeyboardEvent && e.key !== 'Escape')) return;
    menu = null;
    if (e instanceof KeyboardEvent) {
      e.preventDefault();
      e.stopPropagation();
    }
  }

  /**
   * Le presse-papiers de WebKit n'accepte que du PNG : l'image affichée est redessinée dans un
   * canvas, quel que soit son format (un GIF animé n'en garde que l'image courante).
   */
  function png(img: HTMLImageElement): Promise<Blob> {
    const canvas = document.createElement('canvas');
    // Un SVG sans dimensions intrinsèques n'a pas de taille naturelle : on garde celle affichée.
    canvas.width = img.naturalWidth || img.width;
    canvas.height = img.naturalHeight || img.height;
    canvas.getContext('2d')?.drawImage(img, 0, 0, canvas.width, canvas.height);
    return new Promise((resolve, reject) =>
      canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('conversion en PNG impossible'))), 'image/png'),
    );
  }

  async function copyImage(img: HTMLImageElement) {
    menu = null;
    try {
      // WebKit exige que l'élément soit créé dans le geste de l'utilisateur : la conversion est
      // donc passée en promesse, pas attendue avant.
      await navigator.clipboard.write([new ClipboardItem({ 'image/png': png(img) })]);
      app.notify('Image copiée');
    } catch (e) {
      app.notify(`Copie impossible : ${e}`);
    }
  }
</script>

<svelte:window onclick={closeMenu} onkeydowncapture={closeMenu} onblur={() => (menu = null)} />

<div class="body">
  {#if error}
    <p class="muted center">{error}</p>
  {:else if !content}
    <p class="muted center">Chargement…</p>
  {:else if content.dataUrl}
    <div class="image"><img src={content.dataUrl} alt={name} oncontextmenu={openMenu} /></div>
  {:else if rendered !== null}
    <!-- Sans `allow-scripts` ni `allow-same-origin` : la page s'affiche, rien ne s'y exécute.
         Le HTML brut d'un Markdown y est donc inoffensif, sans nettoyage à maintenir. -->
    <iframe title={name} sandbox="" srcdoc={rendered} class:md={content.kind === 'markdown'}></iframe>
  {:else}
    <CodeView text={content.text ?? ''} {name} {line} />
  {/if}
</div>

{#if menu}
  {@const img = menu.img}
  <div class="menu" role="menu" style:left="{menu.x}px" style:top="{menu.y}px">
    <button role="menuitem" onclick={() => void copyImage(img)}>Copier l'image</button>
  </div>
{/if}

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
  .menu {
    position: fixed; z-index: 90; min-width: 180px; padding: 4px; border-radius: 8px;
    background: var(--bg-raised); border: 1px solid var(--line-strong); box-shadow: var(--shadow);
  }
  .menu button {
    display: block; width: 100%; padding: 6px 10px; border: 0; border-radius: var(--radius-sm);
    background: none; text-align: left; color: var(--ink);
  }
  .menu button:hover { background: var(--bg-hover); }
</style>
