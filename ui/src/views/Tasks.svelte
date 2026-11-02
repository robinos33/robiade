<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { ago } from '../lib/format';
  import type { Issue, IssueAction, Task, TaskStatus } from '../lib/types';
  import Icon from '../components/Icon.svelte';
  import IssueActionConfirm from '../components/IssueActionConfirm.svelte';
  import IssueCreateDialog from '../components/IssueCreateDialog.svelte';
  import IssueStartDialog from '../components/IssueStartDialog.svelte';

  /** Issue sur laquelle démarrer un agent, avec le dépôt local de son projet. */
  let starting = $state<{ issue: Issue; root: string } | null>(null);
  /** Action sur une issue, en attente de confirmation. */
  let acting = $state<{ issue: Issue; action: IssueAction } | null>(null);
  /** Projet GitLab où créer une issue, le temps de la saisie. */
  let creating = $state<string | null>(null);
  /** Incrémenté pour relire les issues, après une création. */
  let reload = $state(0);
  /** Une issue fermée ici reste affichée jusqu'à la prochaine lecture, pour pouvoir la rouvrir. */
  function replaced(updated: Issue) {
    issues = issues?.map((i) => (i.webUrl === updated.webUrl ? updated : i)) ?? null;
  }
  /** Dépôt local du projet GitLab de l'issue, s'il est connu de Robiade. */
  const localRepo = (i: Issue) =>
    app.projects.find((p) => p.gitlabProject?.toLowerCase() === i.project.toLowerCase())?.root ?? null;

  const COLUMNS: { status: TaskStatus; label: string }[] = [
    { status: 'todo', label: 'À faire' },
    { status: 'doing', label: 'En cours' },
    { status: 'done', label: 'Fait' },
  ];
  const NEXT: Record<TaskStatus, TaskStatus> = { todo: 'doing', doing: 'done', done: 'todo' };

  let title = $state('');
  /** `AAAA-MM-JJ`, vide tant qu'aucune échéance n'est choisie. */
  let dueOn = $state('');

  /** Issues du projet GitLab sélectionné, ou celles qui me sont assignées en vue globale. */
  let issues = $state<Issue[] | null>(null);
  let issuesError = $state<string | null>(null);
  const issuesScope = $derived(app.project === null ? 'assigned' : app.selectedGitlabProject);

  $effect(() => {
    const scope = issuesScope;
    void reload;
    issues = null;
    issuesError = null;
    if (scope === null) return;
    api.issues(scope === 'assigned' ? null : scope).then(
      (found) => issuesScope === scope && (issues = found),
      (e) => issuesScope === scope && (issuesError = String(e)),
    );
  });

  const projectName = (root: string | null) => app.projects.find((p) => p.root === root)?.name ?? null;

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (!title.trim()) return;
    try {
      await api.createTask(title, app.project, dueOn || null);
      title = '';
      dueOn = '';
      await app.refreshTasks();
    } catch (err) {
      app.notify(String(err));
    }
  }

  async function move(t: Task, status: TaskStatus) {
    await api.updateTask(t.id, { status }).catch((e) => app.notify(String(e)));
    await app.refreshTasks();
  }

  async function schedule(t: Task, day: string) {
    await api.scheduleTask(t.id, day || null).catch((e) => app.notify(String(e)));
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
      <input class="field date" type="date" title="Échéance, facultative" aria-label="Échéance" bind:value={dueOn} />
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
                    <input
                      class="due-on"
                      class:unset={t.dueOn === null}
                      class:due={app.isDue(t)}
                      type="date"
                      title={app.isDue(t) ? (t.dueOn === app.today ? 'Échéance aujourd’hui' : 'Échéance dépassée') : 'Échéance'}
                      aria-label="Échéance"
                      value={t.dueOn ?? ''}
                      onchange={(e) => schedule(t, e.currentTarget.value)}
                    />
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
        {#if issuesScope !== null && issuesScope !== 'assigned'}
          {@const project = issuesScope}
          <button class="btn ghost new" title="Créer une issue sur GitLab, relue avant envoi" onclick={() => (creating = project)}>
            Nouvelle issue
          </button>
        {/if}
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
            <li class="issue" class:closed={i.state === 'closed'}>
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
                <span class="due num" class:late={i.dueDate < app.today} title="Échéance">{i.dueDate}</span>
              {/if}
              <button class="btn ghost act" title="Modifier les assignés, après confirmation" onclick={() => (acting = { issue: i, action: { kind: 'assign', assignees: i.assignees } })}>
                Assignés…
              </button>
              <button class="btn ghost act" title="Modifier les labels, après confirmation" onclick={() => (acting = { issue: i, action: { kind: 'label', labels: i.labels } })}>
                Labels…
              </button>
              {#if i.state === 'opened'}
                <button class="btn ghost act" title="Fermer l'issue sur GitLab, après confirmation" onclick={() => (acting = { issue: i, action: { kind: 'close' } })}>
                  Fermer
                </button>
              {:else}
                <button class="btn ghost act shown" title="Rouvrir l'issue sur GitLab, après confirmation" onclick={() => (acting = { issue: i, action: { kind: 'reopen' } })}>
                  Rouvrir
                </button>
              {/if}
              {#if localRepo(i) && i.state === 'opened'}
                {@const root = localRepo(i) ?? ''}
                <button class="btn ghost start" title="Worktree, branche et prompt à relire" onclick={() => (starting = { issue: i, root })}>
                  Démarrer un agent
                </button>
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

{#if acting}
  <IssueActionConfirm issue={acting.issue} action={acting.action} ondone={replaced} onclose={() => (acting = null)} />
{/if}

{#if creating}
  <IssueCreateDialog project={creating} oncreated={() => reload++} onclose={() => (creating = null)} />
{/if}

{#if starting}
  <IssueStartDialog issue={starting.issue} projectRoot={starting.root} onclose={() => (starting = null)} />
{/if}

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .scroll { overflow-y: auto; padding: 20px 24px 40px; display: flex; flex-direction: column; gap: 24px; }
  form { display: flex; gap: 8px; max-width: 720px; }
  form .field { height: 34px; }
  form .btn { height: 34px; }
  form .date { flex: none; width: auto; }
  .board { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 16px; align-items: start; }
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
  .meta { align-items: center; min-width: 0; }
  .meta > span { white-space: nowrap; }
  .meta .scope { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .due-on {
    margin-left: auto; height: 18px; padding: 0 5px; border: 0; border-radius: 9px;
    background: none; color: var(--ink-2); font: inherit; font-variant-numeric: tabular-nums;
  }
  .due-on:hover, .due-on:focus { background: var(--bg-hover); }
  .due-on.unset { opacity: 0; }
  .task:hover .due-on.unset, .due-on.unset:focus { opacity: 1; }
  .due-on.due { color: var(--danger); background: var(--danger-soft); font-weight: 600; }
  .del { opacity: 0; height: 22px; padding: 0 5px; color: var(--ink-3); }
  .task:hover .del { opacity: 1; }
  .none { padding: 4px 2px; }
  .team { border-top: 1px solid var(--line); padding-top: 18px; }
  .team p { margin: 0; }
  .team h2 span { color: var(--ink-3); margin-left: 4px; }
  .team h2 { display: flex; align-items: center; }
  .new { margin-left: auto; height: 24px; padding: 0 8px; font-size: 12px; text-transform: none; letter-spacing: 0; }
  .issues { gap: 0; }
  .issue { display: flex; align-items: flex-start; gap: 10px; padding: 8px 6px; border-bottom: 1px solid var(--line); }
  .ref { color: var(--ink-3); font-size: 12px; width: 52px; flex: none; padding-top: 1px; }
  .link { border: 0; background: none; padding: 0; text-align: left; color: var(--ink); }
  .link:hover { text-decoration: underline; }
  .label { padding: 0 6px; border-radius: 8px; border: 1px solid var(--line-strong); color: var(--ink-2); }
  .due { flex: none; font-size: 12px; color: var(--ink-2); }
  .due.late { color: var(--danger); }
  .error-text { color: var(--danger); }
  .start, .act { flex: none; height: 24px; padding: 0 8px; font-size: 12px; opacity: 0; }
  .issue:hover .start, .issue:focus-within .start, .issue:hover .act, .issue:focus-within .act, .act.shown { opacity: 1; }
  .closed .link { color: var(--ink-3); text-decoration: line-through; }
</style>
