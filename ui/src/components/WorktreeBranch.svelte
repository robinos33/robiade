<script lang="ts">
  import type { GitWorktree } from '../lib/types';
  import Icon from './Icon.svelte';

  /** `git` est `null` tant que le worktree, connu par l'historique, n'a pas été relu par git. */
  let { git }: { git: GitWorktree | null } = $props();

  const title = $derived(git === null ? 'pas encore relu par git' : (git.branch ?? 'tête détachée'));
</script>

<span class="branch" {title}>
  <Icon name="branch" size={12} />
  {#if git === null}
    <span class="none">—</span>
  {:else if git.branch}
    <!-- Coupée par le début : la fin distingue les branches. `bdi` garde les `/` en place dans le `rtl`. -->
    <span class="tail mono" dir="rtl"><bdi>{git.branch}</bdi></span>
  {:else}
    <span class="tail detached">tête détachée</span>
  {/if}
</span>

<style>
  .branch { display: flex; align-items: center; gap: 4px; min-width: 0; color: var(--ink-2); }
  .branch :global(svg) { flex: none; color: var(--ink-3); }
  .tail { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .detached { font-style: italic; font-size: 12px; color: var(--ink-3); }
  .none { color: var(--ink-3); }
</style>
