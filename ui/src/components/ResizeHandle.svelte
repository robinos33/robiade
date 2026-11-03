<!--
  Poignée posée sur la bordure entre un panneau latéral et la zone principale. Elle se place en
  absolu : elle doit être un enfant direct du panneau, `position: relative` et sans défilement
  (sinon elle défilerait avec le contenu). Le panneau applique `width` lui-même, par une variable
  CSS dont le repli est sa largeur par défaut.
-->
<script lang="ts">
  import { panelDrag } from '../lib/panelDrag.svelte';

  type Props = {
    /** Nom du panneau, qui forme sa clé dans localStorage. */
    id: string;
    /** Bord du panneau qui porte la poignée : `right` pour un panneau de gauche, `left` pour un panneau de droite. */
    edge: 'left' | 'right';
    min: number;
    /** Largeur au-delà de laquelle la zone principale passerait sous son minimum ; `Infinity` tant qu'elle n'est pas mesurée. */
    max: number;
    label: string;
    /** Largeur choisie, bornée ; `null` laisse la largeur par défaut de la feuille de style. */
    width?: number | null;
  };
  let { id, edge, min, max, label, width = $bindable(null) }: Props = $props();

  /** Pas des flèches ← et →, en pixels. */
  const STEP = 16;
  const storageKey = $derived(`robiade.panel.${id}`);
  const sign = $derived(edge === 'right' ? 1 : -1);

  function read(key: string): number | null {
    try {
      const raw = localStorage.getItem(key);
      const value = raw === null ? NaN : Number(raw);
      return Number.isFinite(value) && value > 0 ? value : null;
    } catch {
      return null;
    }
  }
  function save(value: number | null) {
    try {
      if (value === null) localStorage.removeItem(storageKey);
      else localStorage.setItem(storageKey, String(value));
    } catch {
      /* stockage indisponible : la largeur vaut pour ce lancement seulement */
    }
  }

  /**
   * Largeur choisie, telle que retenue. Les bornes ne s'appliquent qu'à l'affichage : après un
   * passage sur un écran plus petit, le panneau retrouve sa largeur quand la place revient.
   */
  // svelte-ignore state_referenced_locally (un panneau garde le même `id` toute sa vie)
  let chosen = $state<number | null>(read(storageKey));
  const clamp = (w: number) => Math.round(Math.max(min, Math.min(max, w)));
  $effect.pre(() => {
    width = chosen === null ? null : clamp(chosen);
  });

  let node: HTMLDivElement;
  /** Largeur rendue du panneau, y compris quand elle vient de la feuille de style. */
  let actual = $state(0);
  $effect(() => {
    const panel = node.parentElement!;
    const observer = new ResizeObserver(() => (actual = panel.getBoundingClientRect().width));
    observer.observe(panel);
    return () => observer.disconnect();
  });

  /** Point de départ du glisser en cours ; `null` hors glisser. */
  let drag: { x: number; width: number } | null = null;
  let dragging = $state(false);

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    // La capture garde le glisser quand le pointeur passe au-dessus du terminal ou d'un iframe.
    node.setPointerCapture(e.pointerId);
    drag = { x: e.clientX, width: node.parentElement!.getBoundingClientRect().width };
    dragging = panelDrag.active = true;
    document.body.classList.add('panel-resizing');
  }
  function move(e: PointerEvent) {
    if (drag) chosen = clamp(drag.width + sign * (e.clientX - drag.x));
  }
  function end() {
    if (!drag) return;
    drag = null;
    dragging = panelDrag.active = false;
    document.body.classList.remove('panel-resizing');
    save(chosen);
  }
  // Panneau fermé en plein glisser : les terminaux ne doivent pas rester suspendus.
  $effect(() => end);

  function reset() {
    chosen = null;
    save(null);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    // Les flèches déplacent la poignée : → élargit un panneau de gauche et rétrécit un panneau de droite.
    chosen = clamp((width ?? actual) + sign * (e.key === 'ArrowRight' ? STEP : -STEP));
    save(chosen);
  }
</script>

<!-- Un séparateur focusable est un widget pour ARIA, ce que la règle de Svelte ignore. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  bind:this={node}
  class="handle {edge}"
  class:dragging
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={Math.round(width ?? actual)}
  aria-valuemin={min}
  aria-valuemax={Number.isFinite(max) ? Math.max(min, Math.round(max)) : undefined}
  tabindex="0"
  title="Glisser pour redimensionner, double-clic pour revenir à la largeur par défaut"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={end}
  onlostpointercapture={end}
  ondblclick={reset}
  {onkeydown}
></div>

<style>
  .handle {
    position: absolute; top: 0; bottom: 0; z-index: 20; width: 8px;
    cursor: col-resize; touch-action: none; outline: none;
  }
  .right { right: -4px; }
  .left { left: -4px; }
  /* Trait centré sur la bordure du panneau : il en recouvre le pixel, quel que soit le côté. */
  .handle::after {
    content: ''; position: absolute; top: 0; bottom: 0; left: 3px; width: 2px;
    background: transparent; transition: background 0.1s;
  }
  .handle:hover::after, .handle:focus-visible::after, .dragging::after { background: var(--accent); }
  :global(body.panel-resizing) { user-select: none; -webkit-user-select: none; }
  :global(body.panel-resizing *) { cursor: col-resize !important; }
  :global(body.panel-resizing iframe) { pointer-events: none; }
</style>
