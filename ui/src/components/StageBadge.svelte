<script lang="ts">
  import type { DbStage } from '../lib/types';

  let { stage }: { stage: DbStage } = $props();

  const LABEL: Record<DbStage, string> = { dev: 'dev', staging: 'staging', prod: 'PROD' };
</script>

<!-- La production en plein rouge, partout où une connexion apparaît : impossible à confondre. -->
<span class="stage {stage}" title={stage === 'prod' ? 'Production : lecture seule imposée' : undefined}>{LABEL[stage]}</span>

<style>
  .stage {
    flex: none; display: inline-flex; align-items: center; height: 18px; padding: 0 6px; border-radius: 4px;
    font: 600 10.5px var(--mono); letter-spacing: 0.02em; border: 1px solid var(--line-strong); color: var(--ink-2);
  }
  .staging { border-color: var(--attention); color: var(--attention); }
  .prod { background: var(--danger); border-color: var(--danger); color: #fff; font-weight: 700; letter-spacing: 0.08em; }
</style>
