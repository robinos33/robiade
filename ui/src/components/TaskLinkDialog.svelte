<script lang="ts">
  import { untrack } from 'svelte';
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import { ago } from '../lib/format';
  import type { Task } from '../lib/types';

  let { task, onclose }: { task: Task; onclose: () => void } = $props();

  /** Projet GitLab du dépôt de la tâche : `#12` s'y lit. */
  const gitlabProject = $derived(app.projects.find((p) => p.root === task.projectPath)?.gitlabProject ?? null);
  const sameProject = (project: string) => project.toLowerCase() === gitlabProject?.toLowerCase();

  // Valeurs à l'ouverture : la saisie part de là, et seul ce qui en diffère est enregistré.
  const { initialIssue, initialSession } = untrack(() => {
    const linked = task.issue;
    const label = linked ? (sameProject(linked.project) ? `#${linked.iid}` : `${linked.project}#${linked.iid}`) : '';
    return { initialIssue: label, initialSession: task.sessionId ?? '' };
  });
  let issue = $state(initialIssue);
  let sessionId = $state(initialSession);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let input: HTMLInputElement | undefined = $state();

  $effect(() => input?.focus());

  /** Sessions du dépôt de la tâche (de tous, pour une tâche globale), les plus récentes d'abord. */
  const sessions = $derived.by(() => {
    const inScope = (root: string) => task.projectPath === null || root === task.projectPath;
    const list = app.conversations.filter((s) => inScope(s.projectRoot)).slice(0, 50);
    // La session liée reste proposée même si elle est sortie de la liste, pour ne pas la perdre en enregistrant.
    const linked = app.history.find((s) => s.sessionId === task.sessionId);
    return linked && !list.includes(linked) ? [linked, ...list] : list;
  });

  async function save() {
    busy = true;
    error = null;
    try {
      if (issue.trim() !== initialIssue) await api.linkTaskIssue(task.id, issue.trim() || null);
      if (sessionId !== initialSession) await api.linkTaskSession(task.id, sessionId || null);
      await app.refreshTasks();
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose();
    } else if (e.key === 'Enter' && !busy) {
      e.preventDefault();
      void save();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation">
  <div class="dialog" role="dialog" aria-labelledby="task-link-title">
    <h2 id="task-link-title">Lier « {task.title} »</h2>
    <div class="grid">
      <label for="task-link-issue">Issue</label>
      <input
        id="task-link-issue"
        class="field mono"
        bind:this={input}
        bind:value={issue}
        spellcheck="false"
        placeholder={gitlabProject ? `#12, groupe/projet#12 ou son URL` : 'groupe/projet#12 ou son URL'}
      />
      <label for="task-link-session">Session</label>
      <select id="task-link-session" class="field" bind:value={sessionId}>
        <option value="">Aucune</option>
        {#each sessions as s (s.sessionId)}
          <option value={s.sessionId}>{s.title} · {task.projectPath === null ? `${s.projectName} · ` : ''}{ago(s.lastActivityAt, app.now)}</option>
        {/each}
      </select>
    </div>
    <p class="muted note">
      L'issue est seulement référencée : son titre et son état restent sur {app.forgeName}. Vider un champ retire le lien.
    </p>
    {#if error}<p class="error">{error}</p>{/if}
    <footer>
      <button class="btn" onclick={onclose}>Annuler</button>
      <button class="btn primary" disabled={busy} onclick={() => void save()}>{busy ? 'Enregistrement…' : 'Enregistrer'}</button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 18vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(560px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 12px;
  }
  h2 { margin: 0; font-size: 15px; overflow-wrap: anywhere; }
  .grid { display: grid; grid-template-columns: 70px 1fr; gap: 8px 12px; align-items: center; }
  label { color: var(--ink-3); font-size: 12.5px; }
  .field { height: 30px; min-width: 0; }
  .note { margin: 0; font-size: 12px; }
  .error { margin: 0; color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
</style>
