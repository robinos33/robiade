<script lang="ts" module>
  import { api } from '../lib/api';
  import type { ModelOption } from '../lib/types';

  /** Une seule demande pour tous les agents ; un échec se retente à l'affichage suivant. */
  let catalog: Promise<ModelOption[]> | null = null;
  function loadModels(): Promise<ModelOption[]> {
    catalog ??= api.agentModels().catch((e) => {
      catalog = null;
      throw e;
    });
    return catalog;
  }
</script>

<script lang="ts">
  import { app } from '../lib/state.svelte';
  import type { Agent, AgentStatus } from '../lib/types';

  /** Lignes Modèle et Effort du panneau Session, à placer dans un `<dl>`. */
  let { agent }: { agent: Agent } = $props();

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
  /** Entrée « défaut » de l'écran `/model` : aucun flag, le réglage de Claude Code s'applique. */
  const DEFAULT = 'default';

  let models: ModelOption[] = $state([]);
  let unreadable = $state('');
  let relaunching = $state(false);
  // Remet les listes sur le choix enregistré après un refus.
  let reset = $state(0);

  $effect(() => {
    loadModels().then(
      (m) => ((models = m), (unreadable = '')),
      (e) => (unreadable = `Liste des modèles de Claude Code illisible : ${e}`),
    );
  });

  const why = $derived(WHY[agent.status]);
  const disabled = $derived(relaunching || why !== '' || unreadable !== '');
  const hint = $derived(
    why || unreadable || 'Relance l’agent sur sa session, avec --model et --effort. Rien n’est écrit dans les réglages de Claude Code.',
  );
  const entry = (value: string | null) => models.find((m) => m.value === (value ?? DEFAULT));
  /** Efforts du modèle visé : celui choisi, sinon l'entrée « défaut ». */
  const efforts = $derived(entry(agent.chosenModel)?.efforts ?? []);
  // Un choix enregistré que la liste ne propose plus (autre compte, autre version) reste affiché.
  const strayModel = $derived(agent.chosenModel !== null && models.length > 0 && !entry(agent.chosenModel));
  const strayEffort = $derived(agent.chosenEffort !== null && !efforts.includes(agent.chosenEffort));
  // Le modèle en service est dans la barre de jauges, au-dessus du terminal ; l'effort ne s'affiche
  // qu'ici. S'il diffère du choix (un `/effort` tapé dans le terminal), il est montré à côté.
  const effortDiffers = $derived(agent.chosenEffort !== null && agent.effort !== null && agent.effort !== agent.chosenEffort);
  /** Sans choix, l'effort en service est celui du défaut : il le nomme. Avec un choix, plus rien ne le dit. */
  const effortDefault = $derived(agent.chosenEffort === null && agent.effort !== null ? `${agent.effort} (défaut)` : 'défaut');
  const noEffort = $derived(models.length > 0 && efforts.length === 0);

  async function choose(model: string | null, effort: string | null) {
    // Un effort que le nouveau modèle n'accepte pas est abandonné plutôt que refusé.
    if (effort !== null && !(entry(model)?.efforts ?? []).includes(effort)) effort = null;
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

  const value = (e: Event) => {
    const v = (e.currentTarget as HTMLSelectElement).value;
    return v === '' || v === DEFAULT ? null : v;
  };
</script>

{#key reset}
  <dt>Modèle</dt>
  <dd class="pick" title={hint}>
    <select aria-label="Modèle" {disabled} value={agent.chosenModel ?? DEFAULT} onchange={(e) => choose(value(e), agent.chosenEffort)}>
      {#if !entry(null)}<option value={DEFAULT}>défaut</option>{/if}
      {#if strayModel}<option value={agent.chosenModel ?? ''}>{agent.chosenModel}</option>{/if}
      {#each models as m (m.value)}<option value={m.value} title={m.description}>{m.displayName}</option>{/each}
    </select>
  </dd>
  <dt>Effort</dt>
  <dd class="pick" title={noEffort && !why && !unreadable ? 'Ce modèle n’a pas de niveau d’effort' : hint}>
    {#if effortDiffers}<span class="mono effective">{agent.effort}</span>{/if}
    <select
      aria-label="Effort"
      disabled={disabled || (noEffort && agent.chosenEffort === null)}
      value={agent.chosenEffort ?? ''}
      onchange={(e) => choose(agent.chosenModel, value(e))}
    >
      <option value="">{effortDefault}</option>
      {#if strayEffort}<option value={agent.chosenEffort ?? ''}>{agent.chosenEffort}</option>{/if}
      {#each efforts as l (l)}<option value={l}>{l}</option>{/each}
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
