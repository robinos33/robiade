<script lang="ts">
  import { untrack } from 'svelte';
  import { app } from '../lib/state.svelte';
  import { api } from '../lib/api';
  import type { DbConnection, DbProbe, DbTarget } from '../lib/types';
  import DbConnectionDialog from '../components/DbConnectionDialog.svelte';
  import StageBadge from '../components/StageBadge.svelte';

  let connections = $state<DbConnection[] | null>(null);
  let error = $state<string | null>(null);
  /** `null` : nouvelle connexion ; `undefined` : formulaire fermé. */
  let editing = $state<DbConnection | null | undefined>(undefined);
  let deleting = $state<string | null>(null);
  let results = $state<Record<string, { probe?: DbProbe; error?: string; busy?: boolean }>>({});
  /** Production à tester : la cible est résolue et relue avant d'ouvrir une session. */
  let confirming = $state<{ connection: DbConnection; target?: DbTarget; error?: string } | null>(null);

  async function refresh() {
    try {
      connections = await api.dbConnections();
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => void untrack(() => refresh()));

  /** Celles du projet sélectionné et les globales ; toutes en vue globale. */
  const visible = $derived(
    (connections ?? []).filter((c) => app.project === null || c.projectPath === null || c.projectPath === app.project),
  );

  const scopeLabel = $derived(
    app.project === null ? 'tous les projets' : app.projects.find((p) => p.root === app.project)?.name,
  );

  /** Ce que la ligne dit de la cible, sans rien résoudre. */
  function where(c: DbConnection): string {
    const host = c.fields.find((f) => f.part === 'host')?.source;
    if (host?.kind === 'literal') return host.value;
    const s = host ?? c.fields.find((f) => f.part === 'url')?.source;
    if (s?.kind === 'vault') return `Vault ${s.mount}/${s.path}#${s.key}`;
    if (s?.kind === 'keychain') return `trousseau ${s.service}`;
    return '';
  }

  const targetLabel = (t: DbTarget) => `${t.user ? `${t.user}@` : ''}${t.host}:${t.port}${t.database ? `/${t.database}` : ''}`;

  async function test(c: DbConnection) {
    confirming = null;
    results[c.id] = { busy: true };
    try {
      results[c.id] = { probe: await api.testDbConnection(c.id) };
    } catch (e) {
      results[c.id] = { error: String(e) };
    }
  }

  async function askTest(c: DbConnection) {
    if (c.stage !== 'prod') return void test(c);
    confirming = { connection: c };
    try {
      const target = await api.resolveDbTarget(c.id);
      if (confirming?.connection.id === c.id) confirming = { connection: c, target };
    } catch (e) {
      if (confirming?.connection.id === c.id) confirming = { connection: c, error: String(e) };
    }
  }

  async function remove(c: DbConnection) {
    if (deleting !== c.id) return void (deleting = c.id);
    deleting = null;
    try {
      await api.deleteDbConnection(c.id);
      await refresh();
    } catch (e) {
      app.notify(`Suppression impossible : ${e}`);
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && confirming) {
      e.preventDefault();
      confirming = null;
    }
  }
</script>

<svelte:window {onkeydown} />

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Bases de données</h1>
    <span class="muted">{scopeLabel}</span>
    <span class="spacer"></span>
    <button class="btn" onclick={() => (editing = null)}>Nouvelle connexion</button>
  </header>

  <div class="scroll">
    {#if error}<div class="error"><strong>Connexions illisibles.</strong> <span>{error}</span></div>{/if}
    {#if connections !== null && visible.length === 0}
      <p class="muted">Aucune connexion{app.project === null ? '' : ' pour ce projet'}.</p>
    {/if}
    <ul class="list">
      {#each visible as c (c.id)}
        {@const r = results[c.id]}
        <li class="row" class:prod={c.stage === 'prod'}>
          <div class="line">
            <StageBadge stage={c.stage} />
            <span class="name ellipsis">{c.name}</span>
            <span class="tag">PostgreSQL</span>
            {#if c.readOnly}<span class="tag">lecture seule</span>{/if}
            <span class="mono muted ellipsis where" title={where(c)}>{where(c)}</span>
            <div class="actions">
              <button class="btn small" class:danger={c.stage === 'prod'} disabled={r?.busy} onclick={() => void askTest(c)}>
                {r?.busy ? 'Test…' : 'Tester'}
              </button>
              <button class="btn small" onclick={() => (editing = c)}>Modifier</button>
              <button class="btn small" onclick={() => void remove(c)} onblur={() => deleting === c.id && (deleting = null)}>
                {deleting === c.id ? 'Confirmer' : 'Supprimer'}
              </button>
            </div>
          </div>
          {#if r?.probe}
            <p class="result ok">
              {c.stage === 'prod' ? 'PROD · ' : ''}{c.name} → <span class="mono">{targetLabel(r.probe.target)}</span>
              · PostgreSQL {r.probe.serverVersion} · connecté en {r.probe.user} sur {r.probe.database}
              · {r.probe.readOnly ? 'lecture seule (vérifiée sur le serveur)' : 'lecture-écriture'}
            </p>
          {:else if r?.error}
            <p class="result error-text">{c.name} : {r.error}</p>
          {/if}
        </li>
      {/each}
    </ul>
  </div>
</section>

{#if editing !== undefined}
  <DbConnectionDialog connection={editing} onclose={() => (editing = undefined)} onsaved={() => void refresh()} />
{/if}

{#if confirming}
  {@const c = confirming.connection}
  <div class="scrim" role="presentation">
    <div class="dialog" role="alertdialog" aria-labelledby="db-test-title">
      <h2 id="db-test-title"><StageBadge stage="prod" /> Se connecter à la production ?</h2>
      <div class="grid">
        <span class="k">Connexion</span>
        <strong>{c.name}</strong>
        <span class="k">Hôte cible</span>
        <strong class="mono">{confirming.target ? targetLabel(confirming.target) : confirming.error ? '—' : 'résolution…'}</strong>
        <span class="k">Accès</span>
        <span>lecture seule, TLS exigé</span>
      </div>
      {#if confirming.error}<p class="error-text">{confirming.error}</p>{/if}
      <footer>
        <button class="btn" onclick={() => (confirming = null)}>Annuler</button>
        <button class="btn danger" disabled={!confirming.target} onclick={() => void test(c)}>Tester sur la PRODUCTION</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .scroll { overflow-y: auto; padding: 22px 24px 40px; display: flex; flex-direction: column; gap: 16px; max-width: 1100px; }
  .list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
  .row { border-bottom: 1px solid var(--line); border-left: 3px solid transparent; padding: 0 10px; }
  .row:hover { background: var(--bg-hover); }
  /* La production se voit avant même de lire la ligne. */
  .row.prod { border-left-color: var(--danger); background: var(--danger-soft); }
  .line { display: flex; align-items: center; gap: 12px; height: 38px; min-width: 0; }
  .name { font-weight: 550; flex: none; max-width: 30%; }
  .prod .name { color: var(--danger); }
  .where { flex: 1; min-width: 0; font-size: 12px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tag { flex: none; font-size: 11px; padding: 0 6px; border-radius: 8px; border: 1px solid var(--line-strong); color: var(--ink-2); }
  .actions { display: flex; gap: 4px; flex: none; }
  .small { height: 24px; padding: 0 8px; font-size: 12px; }
  .danger { background: var(--danger); border-color: transparent; color: #fff; font-weight: 600; }
  .btn:disabled { opacity: 0.5; }
  .result { margin: 0 0 8px; font-size: 12px; overflow-wrap: anywhere; }
  .ok { color: var(--ok); }
  .error-text { color: var(--danger); margin: 0 0 8px; font-size: 12.5px; overflow-wrap: anywhere; }
  .spacer { flex: 1; }
  .error {
    display: flex; flex-direction: column; gap: 4px; padding: 12px 16px;
    border-left: 3px solid var(--danger); border-radius: var(--radius); background: var(--danger-soft);
  }
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 18vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(560px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 12px; border: 2px solid var(--danger);
  }
  h2 { margin: 0; font-size: 15px; display: flex; align-items: center; gap: 10px; }
  .grid { display: grid; grid-template-columns: 90px 1fr; gap: 6px 12px; align-items: baseline; overflow-wrap: anywhere; }
  .k { color: var(--ink-3); font-size: 12.5px; }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
</style>
