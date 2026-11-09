<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import { FORGE_NAMES } from '../lib/forge';
  import ProjectTag from '../components/ProjectTag.svelte';
  import type { ForgeProvider, RepositoryTrunks, SaveOutcome, Settings } from '../lib/types';

  let settings = $state<Settings | null>(null);
  let error = $state<string | null>(null);

  let forgeProvider = $state<ForgeProvider>('gitlab');
  let forgeUrl = $state('');
  let forgeService = $state('');
  let vaultUrl = $state('');
  let vaultService = $state('');

  /** Enregistrement suspendu à la confirmation de l'hôte vers lequel le jeton partira. */
  let pending = $state<{ what: 'forge' | 'vault'; host: string; service: string } | null>(null);
  let busy = $state(false);

  function fill(s: Settings) {
    settings = s;
    forgeProvider = s.forge?.provider ?? 'gitlab';
    forgeUrl = s.forge?.url ?? '';
    forgeService = s.forge?.tokenService ?? '';
    vaultUrl = s.vault?.url ?? '';
    vaultService = s.vault?.tokenService ?? '';
  }

  $effect(() => {
    api.settings().then(fill, (e) => (error = String(e)));
  });

  const forgeChanged = $derived(
    settings !== null
      && (forgeProvider !== (settings.forge?.provider ?? 'gitlab')
        || forgeUrl.trim() !== (settings.forge?.url ?? '')
        || forgeService.trim() !== (settings.forge?.tokenService ?? '')),
  );
  /** Service proposé quand le champ est vide : celui du fournisseur choisi. */
  const forgeDefaultService = $derived(settings?.providers.find((p) => p.id === forgeProvider)?.defaultTokenService ?? '');
  const vaultChanged = $derived(
    settings !== null && (vaultUrl.trim() !== (settings.vault?.url ?? '') || vaultService.trim() !== (settings.vault?.tokenService ?? '')),
  );

  async function save(what: 'forge' | 'vault', confirmedHost: string | null = null) {
    busy = true;
    error = null;
    try {
      let outcome: SaveOutcome;
      if (what === 'forge') {
        const forge = forgeUrl.trim() ? { provider: forgeProvider, url: forgeUrl.trim(), tokenService: forgeService.trim() } : null;
        outcome = await api.saveForgeSettings(forge, confirmedHost);
      } else {
        const vault = vaultUrl.trim() ? { url: vaultUrl.trim(), tokenService: vaultService.trim() } : null;
        outcome = await api.saveVaultSettings(vault, confirmedHost);
      }
      if (outcome.confirmHost) {
        const service = (what === 'forge' ? forgeService : vaultService).trim()
          || (what === 'forge' ? forgeDefaultService : settings?.defaultVaultTokenService) || '';
        pending = { what, host: outcome.confirmHost, service };
      } else if (outcome.settings) {
        pending = null;
        fill(outcome.settings);
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  /** Raccourcis du terminal, en cours d'édition : la barre ne change qu'à l'enregistrement. */
  let rows = $state<{ label: string; text: string }[]>([]);
  let shortcutError = $state<string | null>(null);
  const showShortcuts = () => (rows = app.shortcuts.map(({ label, text }) => ({ label, text })));
  $effect(() => {
    void app.shortcuts;
    showShortcuts();
  });
  const shortcutsChanged = $derived(
    JSON.stringify(rows.map((r) => [r.label.trim(), r.text.trim()]))
      !== JSON.stringify(app.shortcuts.map((s) => [s.label, s.text])),
  );

  function move(i: number, by: -1 | 1) {
    const j = i + by;
    if (j < 0 || j >= rows.length) return;
    [rows[i], rows[j]] = [rows[j]!, rows[i]!];
  }

  async function saveShortcuts() {
    shortcutError = null;
    try {
      app.shortcuts = await api.saveShortcuts(rows.map((r) => ({ label: r.label, text: r.text })));
    } catch (e) {
      shortcutError = String(e);
    }
  }

  /** Troncs par dépôt, et leur saisie : noms séparés par des virgules, dans l'ordre de préférence. */
  let trunks = $state<Record<string, RepositoryTrunks | string>>({});
  let trunkInput = $state<Record<string, string>>({});

  const showTrunks = (root: string, t: RepositoryTrunks) => {
    trunks[root] = t;
    trunkInput[root] = t.configured?.join(', ') ?? '';
  };

  $effect(() => {
    for (const p of app.projects) {
      if (p.root in trunks) continue;
      trunks[p.root] = 'Lecture…';
      api.repositoryTrunks(p.root).then((t) => showTrunks(p.root, t), (e) => (trunks[p.root] = String(e)));
    }
  });

  const branchesOf = (raw: string) => raw.split(/[\s,]+/).filter((b) => b !== '');

  async function saveTrunks(root: string) {
    try {
      showTrunks(root, await api.saveRepositoryTrunks(root, branchesOf(trunkInput[root] ?? '')));
    } catch (e) {
      app.notify(String(e));
    }
  }

  /** Couleur d'un dépôt ; `null` : aucune. S'applique partout dès la réponse, sans relance. */
  async function saveColor(root: string, color: string | null) {
    if (app.colorOf(root) === color) return;
    try {
      app.showColors(await api.saveProjectColor(root, color));
    } catch (e) {
      app.notify(String(e));
    }
  }

  const COLOR_NAMES: Record<string, string> = {
    olive: 'Olive', vert: 'Vert', sarcelle: 'Sarcelle', ciel: 'Ciel', bleu: 'Bleu', violet: 'Violet', rose: 'Rose',
  };

  /** Flèches dans le groupe : la teinte voisine est choisie, et reçoit le focus. */
  function onColorKey(e: KeyboardEvent, root: string) {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!step) return;
    e.preventDefault();
    const choices = [null, ...app.palette];
    const next = choices[(choices.indexOf(app.colorOf(root)) + step + choices.length) % choices.length];
    void saveColor(root, next ?? null);
    const group = e.currentTarget as HTMLElement;
    requestAnimationFrame(() => group.querySelector<HTMLElement>('[aria-checked="true"]')?.focus());
  }

  function onkeydown(e: KeyboardEvent) {
    if (pending && e.key === 'Escape') {
      e.preventDefault();
      pending = null;
    }
  }
</script>

<svelte:window {onkeydown} />

<section class="page">
  <header class="page-head" data-tauri-drag-region>
    <h1>Réglages</h1>
    <span class="muted">enregistrés sur ce poste, sans aucun secret</span>
  </header>

  <div class="scroll">
    {#if error}<p class="error-text">{error}</p>{/if}
    {#if settings?.restartNeeded}
      <div class="restart">
        <span>Les réglages de forge ou de Vault s'appliquent au prochain lancement.</span>
        <button class="btn primary" onclick={() => void api.relaunch()}>Relancer</button>
      </div>
    {/if}

    {#if settings}
      <div class="panel">
        <h2 class="section-title">Forge</h2>
        <p class="muted small">
          Une seule instance. Vide : pas de volet Forge (merge requests, issues, activité).
        </p>
        <label>
          <span>Fournisseur</span>
          <select class="field" bind:value={forgeProvider}>
            {#each settings.providers as p (p.id)}<option value={p.id}>{FORGE_NAMES[p.id]}</option>{/each}
          </select>
        </label>
        <label>
          <span>URL de l'instance</span>
          <input class="field mono" bind:value={forgeUrl} placeholder={forgeProvider === 'github' ? 'https://github.com' : 'https://gitlab.example.com'} spellcheck="false" autocomplete="off" />
        </label>
        <label>
          <span>Service du trousseau du jeton</span>
          <input class="field mono" bind:value={forgeService} placeholder={forgeDefaultService} spellcheck="false" autocomplete="off" />
        </label>
        <p class="muted small">
          Le jeton se saisit dans un vrai terminal :
          <code>security add-generic-password -s {forgeService.trim() || forgeDefaultService} -a "$USER" -w</code>
          {#if forgeProvider === 'github'}
            <br />Un jeton fine-grained limité aux dépôts suivis suffit. Lecture et écriture : Contents
            (merge, rebase), Pull requests, Issues, Actions (relancer des jobs) ; lecture : Commit
            statuses. Sur github.com, il part vers <code>api.github.com</code>.
          {/if}
        </p>
        <footer>
          <button class="btn primary" disabled={!forgeChanged || busy} onclick={() => void save('forge')}>Enregistrer</button>
        </footer>
      </div>

      <div class="panel">
        <h2 class="section-title">Vault</h2>
        <p class="muted small">
          Optionnel. Vide : la source « Vault » disparaît des connexions DB. Le jeton vient de
          <code>vault login</code> (<code>~/.vault-token</code>), sinon du trousseau.
        </p>
        <label>
          <span>URL du Vault</span>
          <input class="field mono" bind:value={vaultUrl} placeholder="https://vault.example.com" spellcheck="false" autocomplete="off" />
        </label>
        <label>
          <span>Service du trousseau de secours</span>
          <input class="field mono" bind:value={vaultService} placeholder={settings.defaultVaultTokenService} spellcheck="false" autocomplete="off" />
        </label>
        <footer>
          <button class="btn primary" disabled={!vaultChanged || busy} onclick={() => void save('vault')}>Enregistrer</button>
        </footer>
      </div>
      <div class="panel">
        <h2 class="section-title">Projets</h2>
        <p class="muted small">
          <strong>Couleur</strong> : reprise partout où le projet apparaît (onglets d'agents, barre
          latérale, listes). Une identité, pas un état : un agent qui attend reste signalé à l'accent.
        </p>
        <p class="muted small">
          <strong>Branches de tronc</strong>, par ordre de préférence. La première qui existe sur
          <code>origin</code> sert de base aux worktrees ; une MR qui n'en cible aucune est un wagon,
          mergé sans squash. Vide : la branche par défaut du remote.
        </p>
        <p class="muted small">Les worktrees d'un dépôt partagent ses réglages.</p>
        {#each app.projects as p (p.root)}
          {@const t = trunks[p.root]}
          {@const current = app.colorOf(p.root)}
          <div class="trunk">
            <span class="name" title={p.root}><ProjectTag name={p.name} root={p.root} /></span>
            <div class="colors" role="radiogroup" aria-label="Couleur de {p.name}" tabindex="-1" onkeydown={(e) => onColorKey(e, p.root)}>
              {#each [null, ...app.palette] as c (c ?? '')}
                {@const label = c ? COLOR_NAMES[c] ?? c : 'Aucune'}
                <button
                  class="swatch"
                  class:none={c === null}
                  role="radio"
                  aria-checked={current === c}
                  aria-label={label}
                  title={label}
                  tabindex={current === c ? 0 : -1}
                  onclick={() => void saveColor(p.root, c)}
                ><i style:background={c ? `var(--project-${c})` : null}></i></button>
              {/each}
            </div>
            {#if typeof t === 'object'}
              <input
                class="field mono"
                bind:value={trunkInput[p.root]}
                placeholder={t.remoteDefault ? `défaut du remote : ${t.remoteDefault}` : 'origin/HEAD absent'}
                spellcheck="false"
                autocomplete="off"
                aria-label="Branches de tronc de {p.name}"
                onkeydown={(e) => e.key === 'Enter' && void saveTrunks(p.root)}
              />
              <button
                class="btn"
                disabled={branchesOf(trunkInput[p.root] ?? '').join(',') === (t.configured ?? []).join(',')}
                onclick={() => void saveTrunks(p.root)}
              >Enregistrer</button>
            {:else}
              <span class="muted small span">{t ?? ''}</span>
            {/if}
          </div>
        {:else}
          <p class="muted small">Aucun projet connu : lance un agent dans un dépôt.</p>
        {/each}
      </div>

      <div class="panel">
        <h2 class="section-title">Raccourcis</h2>
        <p class="muted small">
          Sous le terminal d'un agent. Un clic colle le texte au curseur du prompt, sans l'envoyer :
          Entrée part de toi. Une commande slash seule est suivie d'une espace, pour ses arguments.
          Actifs seulement quand l'agent attend un prompt.
        </p>
        {#each rows as row, i (i)}
          <div class="shortcut">
            <input class="field" bind:value={row.label} placeholder="Libellé" maxlength="24" aria-label="Libellé du raccourci {i + 1}" />
            <textarea class="field mono" bind:value={row.text} rows="1" placeholder="/simplify, ou un prompt" spellcheck="false" aria-label="Texte du raccourci {i + 1}"></textarea>
            <div class="row-actions">
              <button class="btn ghost" disabled={i === 0} onclick={() => move(i, -1)} title="Monter" aria-label="Monter">↑</button>
              <button class="btn ghost" disabled={i === rows.length - 1} onclick={() => move(i, 1)} title="Descendre" aria-label="Descendre">↓</button>
              <button class="btn ghost" onclick={() => rows.splice(i, 1)} title="Supprimer" aria-label="Supprimer">✕</button>
            </div>
          </div>
        {:else}
          <p class="muted small">Aucun raccourci : la barre ne s'affiche pas.</p>
        {/each}
        {#if shortcutError}<p class="error-text small">{shortcutError}</p>{/if}
        <footer>
          <button class="btn" disabled={rows.length >= 20} onclick={() => rows.push({ label: '', text: '' })}>Ajouter</button>
          <button class="btn ghost" disabled={!shortcutsChanged} onclick={showShortcuts}>Annuler</button>
          <button class="btn primary" disabled={!shortcutsChanged} onclick={() => void saveShortcuts()}>Enregistrer</button>
        </footer>
      </div>
    {:else if !error}
      <p class="muted">Lecture des réglages…</p>
    {/if}
  </div>
</section>

{#if pending}
  {@const p = pending}
  <div class="scrim" role="presentation">
    <div class="dialog" role="alertdialog" aria-labelledby="host-title">
      <h2 id="host-title">Envoyer le jeton à {p.host} ?</h2>
      <p>
        Robiade enverra à cet hôte le jeton du service <code>{p.service}</code>, et à lui seul. Vérifie
        l'adresse : une faute de frappe l'enverrait à quelqu'un d'autre.
      </p>
      <div class="grid">
        <span class="k">{p.what === 'forge' ? 'Forge' : 'Vault'}</span>
        <span class="mono host">https://{p.host}</span>
      </div>
      <footer>
        <button class="btn" onclick={() => (pending = null)}>Annuler</button>
        <button class="btn primary" disabled={busy} onclick={() => void save(p.what, p.host)}>Envoyer le jeton à {p.host}</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .page { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .scroll { overflow-y: auto; padding: 20px; display: flex; flex-direction: column; gap: 16px; max-width: 720px; }
  .panel {
    display: flex; flex-direction: column; gap: 10px; padding: 16px;
    border: 1px solid var(--line); border-radius: 10px; background: var(--bg-raised);
  }
  .panel .section-title { margin: 0; }
  label { display: grid; grid-template-columns: 220px 1fr; align-items: center; gap: 12px; }
  label span { color: var(--ink-2); font-size: 13px; }
  .small { font-size: 12.5px; margin: 0; }
  code { font-family: var(--mono); font-size: 12px; overflow-wrap: anywhere; }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .restart {
    display: flex; align-items: center; gap: 12px; justify-content: space-between;
    padding: 10px 14px; border-radius: 8px; background: var(--accent-soft);
  }
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 18vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(560px, 92vw); background: var(--bg-raised); border-radius: 12px; box-shadow: var(--shadow);
    padding: 18px; display: flex; flex-direction: column; gap: 12px;
  }
  .dialog h2 { margin: 0; font-size: 15px; overflow-wrap: anywhere; }
  .dialog p { margin: 0; }
  .grid { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; align-items: baseline; }
  .k { color: var(--ink-3); font-size: 12.5px; }
  .host { overflow-wrap: anywhere; }
  .trunk { display: grid; grid-template-columns: 160px auto 1fr auto; align-items: center; gap: 12px; }
  .trunk .span { grid-column: 3 / 5; }
  .colors { display: flex; gap: 2px; }
  /* Un trait, comme la marque de ProjectTag : la palette montre la couleur telle qu'elle s'affiche. */
  .swatch {
    display: grid; place-items: center; width: 20px; height: 24px; padding: 0;
    border: 1px solid transparent; border-radius: 4px; background: none;
  }
  .swatch i { display: block; width: 4px; height: 14px; border-radius: 1px; }
  .swatch.none i { border: 1px dashed var(--ink-3); width: 6px; }
  .swatch:hover { background: var(--bg-hover); }
  .swatch[aria-checked='true'] { border-color: var(--line-strong); background: var(--bg-active); }
  .swatch:focus-visible { outline: 2px solid var(--ink-2); outline-offset: 1px; }
  .shortcut { display: grid; grid-template-columns: 160px 1fr auto; align-items: start; gap: 12px; }
  .shortcut textarea { resize: vertical; min-height: 28px; field-sizing: content; max-height: 160px; }
  .row-actions { display: flex; gap: 2px; }
  .row-actions .btn { width: 28px; padding: 0; justify-content: center; }
  .trunk .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink-2); font-size: 13px; }
</style>
