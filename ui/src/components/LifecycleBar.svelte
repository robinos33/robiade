<script lang="ts">
  import { api } from '../lib/api';
  import { MERGE_STATUS } from '../lib/mergeRequests';
  import type { Agent, Blocker, Lifecycle, LifecycleStep } from '../lib/types';

  /** `lifecycle` remonte la frise lue de cet agent : la barre du terminal y prend sa MR. */
  let { agent, lifecycle = $bindable(null) }: { agent: Agent; lifecycle?: Lifecycle | null } = $props();

  let read = $state<{ id: string; value: Lifecycle } | null>(null);
  let sequence = 0;

  async function load(id: string) {
    const mine = ++sequence;
    try {
      const value = await api.agentLifecycle(id);
      if (mine === sequence) read = { id, value };
    } catch {
      if (mine === sequence) read = null;
    }
  }

  // Relue à chaque fait qui peut la changer : une écriture, une autre session, un changement
  // d'état (en fin de tour, l'agent vient peut-être de créer sa branche ou sa MR). Côté Rust,
  // la MR n'est relue qu'après 30 s : ces appels ne touchent pas la forge.
  const trigger = $derived(`${agent.id}|${agent.sessionId}|${agent.filesModified}|${agent.status}`);
  $effect(() => {
    void trigger;
    void load(agent.id);
  });
  // Pipelines et reviews bougent sans l'agent.
  $effect(() => {
    const id = agent.id;
    const timer = setInterval(() => void load(id), 30_000);
    return () => clearInterval(timer);
  });

  const current = $derived(read?.id === agent.id ? read.value : null);
  $effect(() => {
    lifecycle = current;
  });

  const ORDER: LifecycleStep[] = ['exploration', 'code', 'draft', 'review', 'merged'];
  type Node = { label: string; at: 'done' | 'current' | 'next'; failed: boolean };

  /** Échec : la forme du nœud change, pas seulement sa couleur. */
  const failing = (b: Blocker | null) =>
    !!b && (b.kind === 'conflicts' || (b.kind === 'pipeline' && (b.status === 'failed' || b.status === 'canceled')));

  const nodes = $derived.by((): Node[] => {
    if (!current) return [];
    const { step, ticket, mergeRequest: mr, blocker } = current;
    const closed = step === 'closed';
    // Fermée sans merge : elle prend la place de Revue, et la frise s'arrête là.
    const reached = ORDER.indexOf(closed ? 'review' : step);
    const label = (s: LifecycleStep) =>
      ({
        exploration: 'Exploration',
        code: 'Code',
        draft: 'Brouillon',
        review: closed ? 'Fermée' : 'Revue',
        merged: mr?.wagon ? `Mergée dans ${mr.targetBranch}` : 'Mergée',
        closed: 'Fermée',
      })[s];
    const steps = ORDER.filter((s) => !(closed && s === 'merged')).map((s, i): Node => ({
      label: label(s),
      at: i < reached ? 'done' : i === reached ? 'current' : 'next',
      failed: i === reached && (closed || failing(blocker)),
    }));
    return ticket === null ? steps : [{ label: `Ticket #${ticket}`, at: 'done', failed: false }, ...steps];
  });

  const PIPELINE = { failed: 'Pipeline en échec', canceled: 'Pipeline annulé', manual: 'Job manuel à lancer', running: 'CI en cours', success: 'Pipeline réussi' };

  function describe(b: Blocker): string {
    switch (b.kind) {
      case 'conflicts': return 'Conflits à résoudre';
      case 'pipeline': return PIPELINE[b.status];
      case 'discussions': return 'Discussions non résolues';
      case 'approvals': return b.left > 1 ? `Attend ${b.left} approbations` : 'Attend 1 approbation';
      case 'approvalsUnknown': return 'Approbations inconnues';
      case 'notMergeable': return `Pas encore mergeable : ${MERGE_STATUS[b.status] ?? b.status}`;
      case 'readyToMerge': return 'Prête à merger';
    }
  }
</script>

{#if current}
  <div class="lifecycle" aria-label="Cycle de vie du ticket">
    <ol class="steps">
      {#each nodes as node, i (node.label)}
        {#if i > 0}<li class="link" class:done={node.at !== 'next'} aria-hidden="true"></li>{/if}
        <li class="node {node.at}" class:failed={node.failed} title={node.label} aria-current={node.at === 'current' ? 'step' : undefined}>
          <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
            {#if node.failed}
              <circle cx="6" cy="6" r="5" class="disc" /><path d="M4 4l4 4M8 4l-4 4" class="cross" />
            {:else if node.at === 'current'}
              <circle cx="6" cy="6" r="5" class="ring" /><circle cx="6" cy="6" r="2.6" class="disc" />
            {:else if node.at === 'done'}
              <circle cx="6" cy="6" r="4" class="disc" />
            {:else}
              <circle cx="6" cy="6" r="3.6" class="hollow" />
            {/if}
          </svg>
          <span class="label">{node.label}</span>
        </li>
      {/each}
    </ol>
    {#if current.blocker}
      {@const b = current.blocker}
      <span class="blocker" class:waiting={b.waiting}>
        <span class="who">{b.waiting ? 'En attente' : 'À faire'}</span>
        <span class="what">{describe(b)}</span>
      </span>
    {:else if current.forgeError}
      <span class="blocker waiting" title={current.forgeError}><span class="what">MR illisible</span></span>
    {/if}
  </div>
{/if}

<style>
  /* Dans la bande sous le terminal, qui porte la container query. */
  .lifecycle { flex: 0 1 auto; min-width: 0; display: flex; align-items: center; gap: 16px; }
  .steps { display: flex; align-items: center; flex: 0 1 auto; min-width: 0; margin: 0; padding: 0; list-style: none; }
  .node { display: inline-flex; align-items: center; gap: 5px; flex: none; white-space: nowrap; color: var(--ink-3); }
  .node.done { color: var(--ink-2); }
  .node.current { color: var(--ink); font-weight: 600; }
  .link { flex: 0 0 28px; height: 1px; margin: 0 6px; background: var(--line-strong); }
  .link.done { background: var(--ink-3); }

  svg { flex: none; --c: currentColor; }
  .current svg { --c: var(--info); }
  .failed svg { --c: var(--danger); }
  .disc { fill: var(--c); }
  .ring { fill: none; stroke: var(--c); stroke-width: 1.2; }
  .hollow { fill: none; stroke: var(--line-strong); stroke-width: 1.2; }
  .cross { stroke: var(--bg); stroke-width: 1.6; stroke-linecap: round; }

  .blocker { flex: none; display: inline-flex; align-items: center; gap: 6px; min-width: 0; max-width: 45%; color: var(--ink); }
  .who {
    flex: none; height: 18px; padding: 0 6px; border-radius: 4px; display: inline-flex; align-items: center;
    font-size: 11px; font-weight: 600; color: var(--attention); border: 1px solid var(--attention);
  }
  .what { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .waiting { color: var(--ink-2); }
  .waiting .who { color: var(--ink-3); border-color: var(--line-strong); font-weight: 520; }

  /* Barre étroite : les étapes autres que la courante gardent leur pastille, leur libellé passe au survol. */
  @container (max-width: 720px) {
    .node:not(.current) .label, .who { display: none; }
    .link { flex-basis: 12px; }
  }
</style>
