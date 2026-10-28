<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { ago } from '../lib/format';
  import type { Issue, Task, TaskStatus } from '../lib/types';
  import Icon from '../components/Icon.svelte';

  const COLUMNS: { status: TaskStatus; label: string }[] = [
    { status: 'todo', label: 'À faire' },
    { status: 'doing', label: 'En cours' },
    { status: 'done', label: 'Fait' },
  ];
  const NEXT: Record<TaskStatus, TaskStatus> = { todo: 'doing', doing: 'done', done: 'todo' };

  let title = $state('');

  /** Issues du projet GitLab sélectionné, ou celles qui me sont assignées en vue globale. */
  let issues = $state<Issue[] | null>(null);
  let issuesError = $state<string | null>(null);
  const issuesScope = $derived(app.project === null ? 'assigned' : app.selectedGitlabProject);

  $effect(() => {
    const scope = issuesScope;
    issues = null;
    issuesError = null;
    if (scope === null) return;
    api.issues(scope === 'assigned' ? null : scope).then(
      (found) => issuesScope === scope && (issues = found),
      (e) => issuesScope === scope && (issuesError = String(e)),
    );
  });

  const today = $derived(new Date(app.now).toISOString().slice(0, 10));
  const projectName = (root: string | null) => app.projects.find((p) => p.root === root)?.name ?? null;

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (!title.trim()) return;
    try {
      await api.createTask(title, app.project);
      title = '';
      await app.refreshTasks();
    } catch (err) {
      app.notify(String(err));
    }
  }

  async function move(t: Task, status: TaskStatus) {
    await api.updateTask(t.id, { status }).catch((e) => app.notify(String(e)));
    await app.refreshTasks();
  }

  async function remove(t: Task) {
    await api.deleteTask(t.id).catch((e) => app.notify(String(e)));
    await app.refreshTasks();
  }
</script>

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Tâches</h1>
    <span class="muted">{app.project ? projectName(app.project) : 'Toutes, y compris globales'}</span>
  </header>

  <div class="scroll">
    <form onsubmit={add}>
      <input class="field" maxlength="200" placeholder={app.project ? `Nouvelle tâche dans ${projectName(app.project)}…` : 'Nouvelle tâche globale…'} bind:value={title} />
      <button class="btn primary" disabled={!title.trim()}>Ajouter</button>
    </form>

    <div class="board">
      {#each COLUMNS as col (col.status)}
        {@const items = app.visibleTasks.filter((t) => t.status === col.status)}
        <div class="col">
          <h2 class="section-title">{col.label} <span class="num">{items.length}</span></h2>
          <ul>
            {#each items as t (t.id)}
              <li class="task {t.status}">
                <button class="check" title="Passer à « {COLUMNS.find((c) => c.status === NEXT[t.status])?.label} »" onclick={() => move(t, NEXT[t.status])} aria-label="Changer le statut"></button>
                <div class="body">
                  <span class="t selectable">{t.title}</span>
                  <span class="meta">
                    <span class="scope">{projectName(t.projectPath) ?? 'Globale'}</span>
                    <span>{ago(t.doneAt ?? t.createdAt, app.now)}</span>
                  </span>
                </div>
                <button class="del btn ghost" title="Supprimer" onclick={() => remove(t)}><Icon name="trash" size={13} /></button>
              </li>
            {:else}
              <li class="none muted">—</li>
            {/each}
          </ul>
        </div>
      {/each}
    </div>

    <div class="team">
      <h2 class="section-title">
        Tâches d'équipe · GitLab · {issuesScope === 'assigned' ? 'assignées à moi' : (issuesScope ?? '—')}
        {#if issues}<span class="num">{issues.length}</span>{/if}
      </h2>
      {#if issuesScope === null}
        <p class="muted">Ce projet n'a pas de remote origin sur gitlab.example.com.</p>
      {:else if issuesError}
        <p class="error-text">GitLab illisible : {issuesError}</p>
      {:else if issues === null}
        <p class="muted">Lecture de GitLab…</p>
      {:else}
        <ul class="issues">
          {#each issues as i (i.webUrl)}
            <li class="issue">
              <span class="ref mono">#{i.iid}</span>
              <div class="body">
                <button class="t link" title="Ouvrir dans GitLab" onclick={() => void openUrl(i.webUrl)}>{i.title}</button>
                <span class="meta">
                  {#if issuesScope === 'assigned'}<span class="scope" title={i.project}>{i.project.split('/').pop()}</span>{/if}
                  {#each i.labels as l (l)}<span class="label">{l}</span>{/each}
                  {#if i.assignees.length}<span>{i.assignees.join(', ')}</span>{/if}
                  {#if i.mergeRequests}<span>{i.mergeRequests} MR</span>{/if}
                  <span>{ago(i.updatedAt, app.now)}</span>
                </span>
              </div>
              {#if i.dueDate}
                <span class="due num" class:late={i.dueDate < today} title="Échéance">{i.dueDate}</span>
              {/if}
            </li>
          {:else}
            <li class="none muted">Aucune issue ouverte.</li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
</section>

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .scroll { overflow-y: auto; padding: 20px 24px 40px; display: flex; flex-direction: column; gap: 24px; }
  form { display: flex; gap: 8px; max-width: 720px; }
  form .field { height: 34px; }
  form .btn { height: 34px; }
  .board { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; align-items: start; }
  .col { background: var(--bg-raised); border: 1px solid var(--line); border-radius: var(--radius); padding: 12px; }
  .col h2 span { color: var(--ink-3); margin-left: 4px; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .task {
    display: flex; align-items: flex-start; gap: 10px; padding: 9px 10px;
    border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--bg);
  }
  .check {
    flex: none; width: 16px; height: 16px; margin-top: 1px; padding: 0;
    border: 1.5px solid var(--line-strong); border-radius: 50%; background: none;
  }
  .check:hover { border-color: var(--accent); }
  .doing .check { border-color: var(--info); background: conic-gradient(var(--info) 0 50%, transparent 50% 100%); }
  .done .check { border-color: var(--ok); background: var(--ok); }
  .done .t { color: var(--ink-3); text-decoration: line-through; }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .t { word-break: break-word; }
  .meta { display: flex; gap: 10px; font-size: 11.5px; color: var(--ink-3); }
  .scope { color: var(--ink-2); }
  .del { opacity: 0; height: 22px; padding: 0 5px; color: var(--ink-3); }
  .task:hover .del { opacity: 1; }
  .none { padding: 4px 2px; }
  .team { border-top: 1px solid var(--line); padding-top: 18px; }
  .team p { margin: 0; }
  .team h2 span { color: var(--ink-3); margin-left: 4px; }
  .issues { gap: 0; }
  .issue { display: flex; align-items: flex-start; gap: 10px; padding: 8px 6px; border-bottom: 1px solid var(--line); }
  .ref { color: var(--ink-3); font-size: 12px; width: 52px; flex: none; padding-top: 1px; }
  .link { border: 0; background: none; padding: 0; text-align: left; color: var(--ink); }
  .link:hover { text-decoration: underline; }
  .label { padding: 0 6px; border-radius: 8px; border: 1px solid var(--line-strong); color: var(--ink-2); }
  .due { flex: none; font-size: 12px; color: var(--ink-2); }
  .due.late { color: var(--danger); }
  .error-text { color: var(--danger); }
</style>
