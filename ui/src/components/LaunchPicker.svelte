<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { Agent, AgentStatus } from '../lib/types';

  /** Lignes Modèle et Effort du panneau Session, à placer dans un `<dl>`. */
  let { agent }: { agent: Agent } = $props();

  // Les mêmes listes que `MODEL_CHOICES` et `EFFORT_CHOICES` côté Rust, qui refuse toute autre valeur.
  const MODELS = ['fable', 'opus', 'sonnet', 'haiku'];
  const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'];

  /** Un choix relance l'agent : jamais pendant un tour ni une demande de permission, qu'il couperait. */
  const WHY: Record<AgentStatus, string> = {
    starting: 'L’agent démarre',
    running: 'L’agent travaille : la relance couperait son tour',
    awaitingPermission: 'Une demande de permission est en cours : la relance la couperait',
    awaitingInput: '',
    idle: '',
    failed: 'Le dernier tour a échoué : relance l’agent avant de changer de modèle',
    ended: '',
  };

  let relaunching = $state(false);
  // Remet les listes sur le choix enregistré après un refus.
  let reset = $state(0);

  const why = $derived(WHY[agent.status]);
  const disabled = $derived(relaunching || why !== '');
  const hint = $derived(why || 'Relance l’agent sur sa session, avec --model et --effort. Rien n’est écrit dans les réglages de Claude Code.');
  // Le modèle en service est dans la barre de jauges, au-dessus du terminal ; l'effort ne s'affiche
  // qu'ici. S'il diffère du choix (un `/effort` tapé dans le terminal), il est montré à côté.
  const effortDiffers = $derived(agent.chosenEffort !== null && agent.effort !== null && agent.effort !== agent.chosenEffort);

  async function choose(model: string | null, effort: string | null) {
    relaunching = true;
    try {
      const fresh = await api.chooseAgentLaunch(agent.id, model, effort, 120, 36);
      app.openAgent(fresh.id);
    } catch (e) {
      app.notify(String(e));
      reset++;
    } finally {
      relaunching = false;
    }
  }

  /** Sans choix, l'effort en service est celui du défaut : il le nomme. Avec un choix, plus rien ne le dit. */
  const effortDefault = $derived(agent.chosenEffort === null && agent.effort !== null ? `${agent.effort} (défaut)` : 'défaut');

  const value = (e: Event) => (e.currentTarget as HTMLSelectElement).value || null;
</script>

{#key reset}
  <dt>Modèle</dt>
  <dd class="pick" title={hint}>
    <select
      aria-label="Modèle"
      {disabled}
      value={agent.chosenModel ?? ''}
      onchange={(e) => choose(value(e), agent.chosenEffort)}
    >
      <option value="">défaut</option>
      {#each MODELS as m (m)}<option value={m}>{m}</option>{/each}
    </select>
  </dd>
  <dt>Effort</dt>
  <dd class="pick" title={hint}>
    {#if effortDiffers}<span class="mono effective">{agent.effort}</span>{/if}
    <select
      aria-label="Effort"
      {disabled}
      value={agent.chosenEffort ?? ''}
      onchange={(e) => choose(agent.chosenModel, value(e))}
    >
      <option value="">{effortDefault}</option>
      {#each EFFORTS as l (l)}<option value={l}>{l}</option>{/each}
    </select>
  </dd>
{/key}

<style>
  /* Le `<dl>` du panneau Session est stylé dans Agents.svelte, dont le style ne descend pas ici. */
  dt { color: var(--ink-3); }
  dd { margin: 0; min-width: 0; }
  .pick { display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
  .effective { color: var(--ink-3); }
  select {
    min-width: 0; max-width: 100%; height: 22px; padding: 0 4px; font-size: 12px; text-overflow: ellipsis;
    background: var(--bg-sunken); border: 1px solid var(--line); border-radius: var(--radius-sm); outline: none;
  }
  select:focus-visible { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  select:disabled { opacity: 0.45; }
</style>
