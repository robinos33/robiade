<script lang="ts">
  import { app } from '../lib/state.svelte';

  /**
   * Nom d'un projet, précédé de sa marque de couleur s'il en a une. Partout où le nom s'affiche, il
   * passe par ici : là où est le nom, est la couleur. Le projet se désigne par la racine de son dépôt
   * (`root`) ou par son projet de forge (`forge`), retrouvé parmi les dépôts locaux.
   */
  let { name, root = null, forge = null, title }: {
    name: string;
    root?: string | null;
    forge?: string | null;
    title?: string;
  } = $props();

  const color = $derived(root ? app.colorOf(root) : forge ? app.colorOfForge(forge) : null);
</script>

<!-- Inline : l'ellipse et la couleur du texte restent celles du parent. -->
<span class="project-tag" {title}>{#if color}<i class="mark" style:background="var(--project-{color})" aria-hidden="true"></i>{/if}{name}</span>

<style>
  /* Jamais ronde (le rond porte l'état), jamais un fond plein (la sélection) : un trait vertical. */
  .mark {
    display: inline-block; width: 3px; height: 0.95em; margin-right: 5px;
    border-radius: 1px; vertical-align: -0.12em;
  }
</style>
