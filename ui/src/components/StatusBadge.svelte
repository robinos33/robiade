<script lang="ts">
  import type { AgentStatus } from '../lib/types';

  let { status, compact = false }: { status: AgentStatus; compact?: boolean } = $props();

  // Forme + libellé : l'état ne repose jamais sur la couleur seule.
  const LABELS: Record<AgentStatus, string> = {
    starting: 'Démarrage',
    running: 'En cours',
    awaitingPermission: 'Permission',
    awaitingInput: 'À lire',
    idle: 'En pause',
    failed: 'Échec',
    ended: 'Terminée',
  };
</script>

<span class="badge {status}" class:compact title={LABELS[status]}>
  <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
    {#if status === 'running'}
      <circle cx="6" cy="6" r="3" class="fill" />
      <circle cx="6" cy="6" r="5" class="ring" />
    {:else if status === 'starting'}
      <!-- Arc qui tourne : aucun hook encore reçu. Distinct du point pulsé d'« En cours » et du cercle d'« En pause ». -->
      <path d="M6 2a4 4 0 1 1-4 4" class="stroke spin" />
    {:else if status === 'awaitingPermission'}
      <path d="M6 1.2 11 10.4H1z" class="fill" />
      <rect x="5.4" y="4.4" width="1.2" height="3" rx=".6" class="glyph" />
      <circle cx="6" cy="8.7" r=".7" class="glyph" />
    {:else if status === 'awaitingInput'}
      <circle cx="6" cy="6" r="4.2" class="fill" />
    {:else if status === 'idle'}
      <circle cx="6" cy="6" r="3.8" class="hollow" />
    {:else if status === 'failed'}
      <path d="M3 3l6 6M9 3l-6 6" class="stroke" />
    {:else}
      <path d="M2.5 6h7" class="stroke" />
    {/if}
  </svg>
  <!-- `compact-hide` : ce qu'un conteneur étroit peut masquer en CSS, la pastille et le survol restent. -->
  {#if !compact}<span class="compact-hide">{LABELS[status]}</span>{/if}
</span>

<style>
  .badge {
    --c: var(--ink-3);
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 12px; font-weight: 520; color: var(--c); white-space: nowrap;
  }
  .running, .starting { --c: var(--info); }
  .awaitingPermission { --c: var(--attention); }
  .awaitingInput { --c: var(--accent); }
  .idle { --c: var(--ink-2); }
  .failed { --c: var(--danger); }
  .fill { fill: var(--c); }
  .glyph { fill: var(--bg); }
  .hollow { fill: none; stroke: var(--c); stroke-width: 1.5; }
  .stroke { fill: none; stroke: var(--c); stroke-width: 1.8; stroke-linecap: round; }
  .ring {
    fill: none; stroke: var(--c); stroke-width: 1.2; opacity: 0.5;
    transform-origin: 6px 6px; animation: pulse 1.6s ease-out infinite;
  }
  .spin { transform-origin: 6px 6px; animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes pulse {
    from { transform: scale(0.6); opacity: 0.7; }
    to { transform: scale(1.15); opacity: 0; }
  }
</style>
