<script lang="ts">
  import { app } from '../lib/state.svelte';
  import type { IssueStart } from '../lib/types';

  let {
    heading,
    fromGitlab,
    prepare,
    launch,
    onclose,
  }: {
    heading: string;
    /** Le prompt cite une description venue de GitLab : le rappeler avant l'envoi. */
    fromGitlab: boolean;
    prepare: () => Promise<IssueStart>;
    launch: (branch: string, base: string, prompt: string) => Promise<{ agentId: string; fetched: boolean }>;
    onclose: () => void;
  } = $props();

  let proposal = $state<IssueStart | null>(null);
  let branch = $state('');
  let base = $state('');
  let prompt = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);

  // Même convention que le domaine : `/` du nom de branche remplacé par `-` dans le dossier.
  const dir = $derived(proposal ? `${proposal.repository}/.claude/worktrees/${branch.replaceAll('/', '-')}` : '');

  $effect(() => {
    prepare().then(
      (p) => {
        proposal = p;
        branch = p.branch;
        base = p.bases[0] ?? '';
        prompt = p.prompt;
      },
      (e) => (error = String(e)),
    );
  });

  async function start() {
    busy = true;
    error = null;
    try {
      const started = await launch(branch.trim(), base, prompt);
      if (!started.fetched) app.notify(`${base} n'a pas pu être mis à jour depuis origin : la base locale a servi.`);
      app.openAgent(started.agentId);
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
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation">
  <div class="dialog" role="dialog" aria-labelledby="start-title">
    <h2 id="start-title">{heading}</h2>
    {#if !proposal}
      <p class="muted">{error ? '' : 'Lecture du dépôt…'}</p>
    {:else}
      <div class="grid">
        <label for="branch">Branche</label>
        <input id="branch" class="field mono" bind:value={branch} maxlength="100" spellcheck="false" />
        <label for="base">Depuis</label>
        <select id="base" class="field mono" bind:value={base}>
          {#each proposal.bases as b (b)}<option value={b}>{b}</option>{/each}
        </select>
        <span class="k">Worktree</span>
        <span class="mono path">{dir}</span>
      </div>
      <label class="k" for="prompt">Premier message, envoyé tel quel à l'agent</label>
      <textarea id="prompt" class="field mono" rows="14" bind:value={prompt}></textarea>
      <p class="muted note">
        {#if fromGitlab}La description vient de {app.forgeName} : relis-la, une issue peut contenir des consignes destinées à l'agent.{/if}
        {base} est mis à jour depuis origin avant la création ; aucun hook git ne tourne.
      </p>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    <footer>
      <button class="btn" onclick={onclose}>Annuler</button>
      <button class="btn primary" disabled={!proposal || busy || !branch.trim() || !prompt.trim()} onclick={() => void start()}>
        {busy ? 'Création du worktree…' : 'Créer le worktree et lancer'}
      </button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 8vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(760px, 94vw); max-height: 86vh; overflow: auto; background: var(--bg-raised); border-radius: 12px;
    box-shadow: var(--shadow); padding: 18px; display: flex; flex-direction: column; gap: 12px;
  }
  h2 { margin: 0; font-size: 15px; overflow-wrap: anywhere; }
  .grid { display: grid; grid-template-columns: 80px 1fr; gap: 8px 12px; align-items: center; }
  label, .k { color: var(--ink-3); font-size: 12.5px; }
  .field { height: 30px; }
  textarea.field { height: auto; resize: vertical; padding: 8px 10px; font-size: 12px; line-height: 1.5; }
  .path { font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; }
  .note { margin: 0; font-size: 12px; }
  .error { margin: 0; color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
</style>
