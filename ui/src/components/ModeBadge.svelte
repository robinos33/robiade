<script lang="ts">
  /** Mode de permission de l'agent. Un mode permissif est un signal de sécurité : il doit se voir. */
  let { mode, permissive, compact = false }: { mode: string | null; permissive: boolean; compact?: boolean } =
    $props();

  const LABELS: Record<string, string> = {
    default: 'Demande tout',
    acceptEdits: 'Éditions acceptées',
    plan: 'Plan',
    auto: 'Auto',
    bypassPermissions: 'Sans permission',
  };
  const label = $derived(mode ? (LABELS[mode] ?? mode) : null);
</script>

{#if label && (permissive || !compact)}
  <span class="mode" class:permissive title="Mode de permission : {mode}">
    {#if permissive}<svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true"><path d="M8 2.5 14 13H2zM8 6.5v3M8 11.3v.01" /></svg>{/if}
    {label}
  </span>
{/if}

<style>
  .mode {
    display: inline-flex; align-items: center; gap: 4px; flex: none;
    height: 18px; padding: 0 6px; border-radius: 9px;
    border: 1px solid var(--line-strong); color: var(--ink-2); font-size: 11px;
  }
  .permissive { border-color: color-mix(in srgb, var(--danger) 45%, transparent); background: var(--danger-soft); color: var(--danger); font-weight: 600; }
  svg { fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
</style>
