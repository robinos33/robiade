<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { RepositoryTrunks, SaveOutcome, Settings } from '../lib/types';

  let settings = $state<Settings | null>(null);
  let error = $state<string | null>(null);

  let forgeUrl = $state('');
  let forgeService = $state('');
  let vaultUrl = $state('');
  let vaultService = $state('');

  /** Enregistrement suspendu à la confirmation de l'hôte vers lequel le jeton partira. */
  let pending = $state<{ what: 'forge' | 'vault'; host: string; service: string } | null>(null);
  let busy = $state(false);

  function fill(s: Settings) {
    settings = s;
    forgeUrl = s.forge?.url ?? '';
    forgeService = s.forge?.tokenService ?? '';
    vaultUrl = s.vault?.url ?? '';
    vaultService = s.vault?.tokenService ?? '';
  }

  $effect(() => {
    api.settings().then(fill, (e) => (error = String(e)));
  });

  const forgeChanged = $derived(
    settings !== null && (forgeUrl.trim() !== (settings.forge?.url ?? '') || forgeService.trim() !== (settings.forge?.tokenService ?? '')),
  );
  const vaultChanged = $derived(
    settings !== null && (vaultUrl.trim() !== (settings.vault?.url ?? '') || vaultService.trim() !== (settings.vault?.tokenService ?? '')),
  );

  async function save(what: 'forge' | 'vault', confirmedHost: string | null = null) {
    busy = true;
    error = null;
    try {
      let outcome: SaveOutcome;
      if (what === 'forge') {
        const forge = forgeUrl.trim() ? { provider: 'gitlab' as const, url: forgeUrl.trim(), tokenService: forgeService.trim() } : null;
        outcome = await api.saveForgeSettings(forge, confirmedHost);
      } else {
        const vault = vaultUrl.trim() ? { url: vaultUrl.trim(), tokenService: vaultService.trim() } : null;
        outcome = await api.saveVaultSettings(vault, confirmedHost);
      }
      if (outcome.confirmHost) {
        const service = (what === 'forge' ? forgeService : vaultService).trim()
          || (what === 'forge' ? settings?.defaultForgeTokenService : settings?.defaultVaultTokenService) || '';
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
          <select class="field" disabled><option>GitLab</option></select>
        </label>
        <label>
          <span>URL de l'instance</span>
          <input class="field mono" bind:value={forgeUrl} placeholder="https://gitlab.example.com" spellcheck="false" autocomplete="off" />
        </label>
        <label>
          <span>Service du trousseau du jeton</span>
          <input class="field mono" bind:value={forgeService} placeholder={settings.defaultForgeTokenService} spellcheck="false" autocomplete="off" />
        </label>
        <p class="muted small">
          Le jeton se saisit dans un vrai terminal :
          <code>security add-generic-password -s {forgeService.trim() || settings.defaultForgeTokenService} -a "$USER" -w</code>
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
        <h2 class="section-title">Projets · branches de tronc</h2>
        <p class="muted small">
          Par ordre de préférence. La première qui existe sur <code>origin</code> sert de base aux
          worktrees ; une MR qui n'en cible aucune est un wagon, mergé sans squash. Vide : la branche
          par défaut du remote. Les worktrees d'un dépôt partagent son réglage.
        </p>
        {#each app.projects as p (p.root)}
          {@const t = trunks[p.root]}
          <div class="trunk">
            <span class="name" title={p.root}>{p.name}</span>
            {#if typeof t === 'object'}
              <input
                class="field mono"
                bind:value={trunkInput[p.root]}
                placeholder={t.remoteDefault ? `défaut du remote : ${t.remoteDefault}` : 'origin/HEAD absent'}
                spellcheck="false"
                autocomplete="off"
                onkeydown={(e) => e.key === 'Enter' && void saveTrunks(p.root)}
              />
              <button
                class="btn"
                disabled={branchesOf(trunkInput[p.root] ?? '').join(',') === (t.configured ?? []).join(',')}
                onclick={() => void saveTrunks(p.root)}
              >Enregistrer</button>
            {:else}
              <span class="muted small">{t ?? ''}</span>
            {/if}
          </div>
        {:else}
          <p class="muted small">Aucun projet connu : lance un agent dans un dépôt.</p>
        {/each}
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
  .trunk { display: grid; grid-template-columns: 220px 1fr auto; align-items: center; gap: 12px; }
  .trunk .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink-2); font-size: 13px; }
</style>
