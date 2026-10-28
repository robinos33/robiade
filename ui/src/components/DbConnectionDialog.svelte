<script lang="ts">
  import { untrack } from 'svelte';
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { DbConnection, DbEngine, DbPart, DbSource, DbStage } from '../lib/types';
  import StageBadge from './StageBadge.svelte';
  import VaultPicker from './VaultPicker.svelte';

  let { connection, onclose, onsaved }: {
    connection: DbConnection | null;
    onclose: () => void;
    onsaved: (saved: DbConnection) => void;
  } = $props();

  type Kind = 'none' | DbSource['kind'];

  /** Moteurs qui savent se connecter ; Mongo et Elastic viendront ensuite. */
  const ENGINES: { engine: DbEngine; label: string; port: number }[] = [
    { engine: 'postgres', label: 'PostgreSQL', port: 5432 },
    { engine: 'mariadb', label: 'MariaDB', port: 3306 },
    { engine: 'elasticsearch', label: 'Elasticsearch', port: 9200 },
  ];
  /** Ce que chaque champ veut dire pour Elasticsearch, qui n'a ni base ni DATABASE_URL. */
  const ES_HINTS: Partial<Record<DbPart, string>> = {
    url: 'ELASTICSEARCH_URL, http(s)://',
    database: 'sans objet',
    password: 'ou clé d’API encodée, sans utilisateur',
  };

  const PARTS: { part: DbPart; label: string; secret: boolean; hint: string }[] = [
    { part: 'url', label: 'URL', secret: true, hint: 'DATABASE_URL de Symfony' },
    { part: 'host', label: 'Hôte', secret: false, hint: 'remplace celui de l’URL' },
    { part: 'port', label: 'Port', secret: false, hint: 'port du moteur par défaut' },
    { part: 'database', label: 'Base', secret: false, hint: '' },
    { part: 'user', label: 'Utilisateur', secret: false, hint: '' },
    { part: 'password', label: 'Mot de passe', secret: true, hint: '' },
  ];

  /** Un champ du formulaire : la source choisie, et ce qui a été saisi pour chaque sorte. */
  interface Row { kind: Kind; value: string; service: string; vault: Extract<DbSource, { kind: 'vault' }> | null }

  function rowOf(part: DbPart): Row {
    const s = initial?.fields.find((f) => f.part === part)?.source;
    return {
      kind: s?.kind ?? 'none',
      value: s?.kind === 'literal' ? s.value : '',
      service: s?.kind === 'keychain' ? s.service : '',
      vault: s?.kind === 'vault' ? s : null,
    };
  }

  // Valeurs de départ, figées à l'ouverture : la connexion éditée ne change pas sous le formulaire.
  const initial = untrack(() => $state.snapshot(connection));
  let name = $state(initial?.name ?? '');
  let engine = $state<DbEngine>(initial?.engine ?? 'postgres');
  let stage = $state<DbStage>(initial?.stage ?? 'dev');
  let readOnly = $state(initial?.readOnly ?? true);
  let attached = $state(initial ? initial.projectPath !== null : app.project !== null);
  const projectPath = initial ? initial.projectPath ?? app.project : app.project;
  let rows = $state(Object.fromEntries(PARTS.map(({ part }) => [part, rowOf(part)])) as Record<DbPart, Row>);
  let picking = $state<DbPart | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let input: HTMLInputElement | undefined = $state();

  $effect(() => input?.focus());

  const prod = $derived(stage === 'prod');
  // La production est en lecture seule, sans bascule : la case suit, désactivée.
  $effect(() => {
    if (prod) readOnly = true;
  });

  /** Ce que le bandeau peut dire de la cible avant toute résolution. */
  const hostLabel = $derived.by(() => {
    const host = rows.host;
    if (host.kind === 'literal' && host.value.trim()) return host.value.trim();
    const from = host.kind === 'none' ? rows.url : host;
    if (from.kind === 'vault' && from.vault) return `lu dans Vault : ${from.vault.mount}/${from.vault.path}#${from.vault.key}`;
    if (from.kind === 'keychain' && from.service) return `lu dans le trousseau : ${from.service}`;
    return 'non renseigné';
  });

  const hint = (p: (typeof PARTS)[number]) => (engine === 'elasticsearch' ? ES_HINTS[p.part] ?? p.hint : p.hint);

  /** Secret enregistré en clair : il n'est jamais renvoyé ici, laissé vide il reste inchangé. */
  const storedClear = (part: DbPart) => initial?.fields.some((f) => f.part === part && f.source.kind === 'literal') ?? false;

  const LOCAL_HOSTS = ['localhost', '127.0.0.1', '::1'];
  /** Hôte d'une URL Postgres, MariaDB (`mysql://`) ou Elasticsearch, crochets d'IPv6 retirés. */
  const urlHost = (url: string) =>
    /^(?:postgres(?:ql)?|mysql|mariadb|https?):\/\/(?:[^/]*@)?(\[[^\]]+\]|[^:/?]+)/.exec(url.trim())?.[1]?.replace(/^\[|\]$/g, '');

  /** Même règle que le domaine, pour n'offrir la saisie en clair que là où elle sera acceptée :
   *  dev, et un hôte local connu sans rien résoudre. Rust tranche de toute façon à l'enregistrement. */
  const clearAllowed = $derived.by(() => {
    if (stage !== 'dev') return false;
    const { host, url } = rows;
    if (host.kind === 'literal') return LOCAL_HOSTS.includes(host.value.trim());
    if (host.kind !== 'none' || url.kind !== 'literal') return false;
    if (!url.value && storedClear('url')) return true;
    return LOCAL_HOSTS.includes(urlHost(url.value) ?? '');
  });

  function source(row: Row): DbSource | null {
    if (row.kind === 'literal') return { kind: 'literal', value: row.value };
    if (row.kind === 'keychain') return { kind: 'keychain', service: row.service.trim() };
    if (row.kind === 'vault') return row.vault;
    return null;
  }

  async function save() {
    busy = true;
    error = null;
    try {
      const fields = PARTS.flatMap(({ part }) => {
        const s = source(rows[part]);
        return s ? [{ part, source: s }] : [];
      });
      const saved = await api.saveDbConnection({
        id: initial?.id ?? null,
        name,
        engine,
        stage,
        projectPath: attached ? projectPath : null,
        readOnly,
        fields,
      });
      onsaved(saved);
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      if (picking) picking = null;
      else onclose();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation">
  <div class="dialog" class:prod role="dialog" aria-labelledby="db-title">
    <h2 id="db-title">
      {initial ? 'Modifier la connexion' : 'Nouvelle connexion'}
      <StageBadge {stage} />
    </h2>

    {#if prod}
      <!-- Nom et cible en clair dès le passage en production : on sait toujours où l'on va. -->
      <div class="banner" role="alert">
        <strong>PRODUCTION · lecture seule imposée</strong>
        <span>Connexion <b>{name.trim() || 'sans nom'}</b> → hôte <b class="mono">{hostLabel}</b></span>
      </div>
    {/if}

    <div class="grid">
      <label for="db-name">Nom</label>
      <input id="db-name" class="field" bind:this={input} bind:value={name} maxlength="100" placeholder="Forum staging" />

      <span class="k">Moteur</span>
      <div class="stages" role="radiogroup" aria-label="Moteur">
        {#each ENGINES as e (e.engine)}
          <button class="btn small" class:on={engine === e.engine} role="radio" aria-checked={engine === e.engine} title={`port ${e.port} par défaut`} onclick={() => (engine = e.engine)}>
            {e.label}
          </button>
        {/each}
        <span class="muted">· Mongo plus tard</span>
      </div>

      <span class="k">Environnement</span>
      <div class="stages" role="radiogroup" aria-label="Environnement">
        {#each ['dev', 'staging', 'prod'] as const as s (s)}
          <button class="btn small" class:on={stage === s} class:prod-choice={s === 'prod'} role="radio" aria-checked={stage === s} onclick={() => (stage = s)}>
            {s === 'prod' ? 'PROD' : s}
          </button>
        {/each}
      </div>

      <span class="k">Accès</span>
      <label class="check">
        <input type="checkbox" bind:checked={readOnly} disabled={prod} />
        Lecture seule {#if prod}<span class="muted">(imposée en production)</span>{/if}
      </label>

      {#if projectPath}
        <span class="k">Projet</span>
        <label class="check"><input type="checkbox" bind:checked={attached} /> <span class="mono path">{projectPath}</span></label>
      {/if}
    </div>

    <div class="fields">
      {#each PARTS as p (p.part)}
        {@const row = rows[p.part]}
        <div class="field-row">
          <span class="k" title={hint(p)}>{p.label}</span>
          <select class="field kind" bind:value={row.kind} onchange={() => (picking = row.kind === 'vault' && !row.vault ? p.part : null)}>
            <option value="none">—</option>
            <option value="literal" disabled={p.secret && !clearAllowed && row.kind !== 'literal'}>
              Valeur{p.secret && !clearAllowed ? ' (dev local seulement)' : ''}
            </option>
            <option value="keychain">Trousseau</option>
            <option value="vault">Vault</option>
          </select>
          {#if row.kind === 'literal' && p.secret}
            <input class="field mono" class:refused={!clearAllowed} type="password" bind:value={row.value} autocomplete="off"
              placeholder={storedClear(p.part) ? 'inchangé' : hint(p)} />
          {:else if row.kind === 'literal'}
            <input class="field mono" bind:value={row.value} placeholder={hint(p)} spellcheck="false" />
          {:else if row.kind === 'keychain'}
            <input class="field mono" bind:value={row.service} placeholder="service du trousseau" spellcheck="false" />
          {:else if row.kind === 'vault'}
            <button class="btn vault mono" onclick={() => (picking = picking === p.part ? null : p.part)}>
              {row.vault ? `${row.vault.mount}/${row.vault.path}#${row.vault.key}` : 'Choisir dans Vault…'}
            </button>
          {:else}
            <span class="muted hint">{hint(p)}</span>
          {/if}
        </div>
        {#if picking === p.part}
          <VaultPicker onpick={(v) => ((row.vault = v), (picking = null))} />
        {/if}
      {/each}
      <p class="muted note" class:warn={!clearAllowed && PARTS.some((p) => p.secret && rows[p.part].kind === 'literal')}>
        {#if clearAllowed}
          Base locale de dev : le mot de passe peut se saisir en clair, il est gardé par Robiade comme dans le
          <span class="mono">.env</span> du projet.
        {:else}
          Une URL ou un mot de passe ne se saisit en clair que pour une connexion dev vers
          <span class="mono">localhost</span> (hôte en clair). Ailleurs : trousseau
          (<span class="mono">security add-generic-password -s &lt;service&gt; -a "$USER" -w</span>, dans un terminal) ou Vault.
        {/if}
      </p>
    </div>

    {#if error}<p class="error">{error}</p>{/if}
    <footer>
      <button class="btn" onclick={onclose}>Annuler</button>
      <button class="btn" class:primary={!prod} class:danger={prod} disabled={busy || !name.trim()} onclick={() => void save()}>
        {busy ? 'Enregistrement…' : prod ? 'Enregistrer en PRODUCTION' : 'Enregistrer'}
      </button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 8vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px); overflow-y: auto;
  }
  .dialog {
    width: min(680px, 94vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 12px; border: 2px solid transparent; margin-bottom: 8vh;
  }
  .dialog.prod { border-color: var(--danger); }
  h2 { margin: 0; font-size: 15px; display: flex; align-items: center; gap: 10px; }
  .banner {
    display: flex; flex-direction: column; gap: 2px; padding: 10px 12px; border-radius: var(--radius);
    background: var(--danger); color: #fff; overflow-wrap: anywhere;
  }
  .banner strong { letter-spacing: 0.06em; font-size: 12px; }
  .grid { display: grid; grid-template-columns: 110px 1fr; gap: 8px 12px; align-items: center; }
  label, .k { color: var(--ink-3); font-size: 12.5px; }
  .check { display: flex; align-items: center; gap: 6px; color: var(--ink); }
  .field { height: 30px; }
  .stages { display: flex; align-items: center; gap: 4px; }
  .small { height: 26px; padding: 0 10px; font-size: 12px; }
  .on { border-color: var(--ink-2); font-weight: 600; }
  .prod-choice.on { background: var(--danger); border-color: var(--danger); color: #fff; }
  .fields { display: flex; flex-direction: column; gap: 6px; }
  .field-row { display: grid; grid-template-columns: 110px 120px 1fr; gap: 8px; align-items: center; }
  .vault { text-align: left; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hint { font-size: 12px; }
  .path { font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; }
  .note { margin: 4px 0 0; font-size: 11.5px; }
  .note.warn { color: var(--danger); }
  .refused { border-color: var(--danger); }
  .error { margin: 0; color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .danger { background: var(--danger); border-color: transparent; color: #fff; font-weight: 600; }
  .btn:disabled { opacity: 0.5; }
</style>
