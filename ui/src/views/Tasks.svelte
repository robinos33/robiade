<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { ago } from '../lib/format';
  import type { Task, TaskStatus } from '../lib/types';
  import Icon from '../components/Icon.svelte';

  const COLUMNS: { status: TaskStatus; label: string }[] = [
    { status: 'todo', label: 'À faire' },
    { status: 'doing', label: 'En cours' },
    { status: 'done', label: 'Fait' },
  ];
  const NEXT: Record<TaskStatus, TaskStatus> = { todo: 'doing', doing: 'done', done: 'todo' };

  let title = $state('');
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
      <h2 class="section-title">Tâches d'équipe · GitLab</h2>
      <p class="muted">Les issues GitLab s'afficheront ici en lecture (GitLab reste la source de vérité). Prévu en v2.</p>
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
</style>
