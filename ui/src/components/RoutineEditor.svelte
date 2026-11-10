<script lang="ts">
  import { untrack } from 'svelte';
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import { dateTime, shortPath } from '../lib/format';
  import type { Routine, RoutineInput, RoutineSavePreview } from '../lib/types';

  /** `routine` : celle qu'on modifie ; `null` pour en créer une. */
  let { routine, onclose, ondone }: {
    routine: Routine | null;
    onclose: () => void;
    ondone: (label: string) => void;
  } = $props();

  const DAYS = ['lun', 'mar', 'mer', 'jeu', 'ven', 'sam', 'dim'];
  const home = $derived(app.env?.home ?? '');

  // Le formulaire part de la routine telle qu'elle était lue à l'ouverture.
  const initial = untrack(() => routine?.form);
  let name = $state(initial?.name ?? '');
  let program = $state(initial?.program ?? '');
  /** Un champ par argument : un argument peut tenir sur plusieurs lignes (`sh -c`). */
  let args = $state<string[]>(initial?.arguments.length ? [...initial.arguments] : ['']);
  let workingDir = $state(initial?.workingDir ?? '');
  let days = $state<number[]>(initial?.days ?? [1, 2, 3, 4, 5]);
  let times = $state((initial?.times ?? ['9:00']).join(', '));

  let preview = $state<RoutineSavePreview | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let nameInput: HTMLInputElement | undefined = $state();
  let programInput: HTMLInputElement | undefined = $state();
  // Le nom d'une routine existante ne change pas : le focus va au programme.
  $effect(() => (routine ? programInput : nameInput)?.focus());

  function trimmedArgs(): string[] {
    let end = args.length;
    while (end > 0 && args[end - 1] === '') end -= 1;
    return args.slice(0, end);
  }

  function input(): RoutineInput {
    return {
      editing: routine?.label ?? null,
      name: name.trim(),
      program: program.trim(),
      // Un dernier champ vide n'est pas un argument ; un vide au milieu en est un (`""`).
      arguments: trimmedArgs(),
      workingDir: workingDir.trim() || null,
      days: [...days].sort(),
      times: times.split(/[,;\s]+/).filter((t) => t !== ''),
    };
  }

  // Toute modification invalide l'aperçu : on n'enregistre que ce qu'on a vu.
  $effect(() => {
    void [name, program, workingDir, times, days.length, ...args, ...days];
    preview = null;
  });

  function toggleDay(d: number) {
    days = days.includes(d) ? days.filter((x) => x !== d) : [...days, d];
  }

  async function showPreview() {
    busy = true;
    error = null;
    try {
      preview = await api.prepareRoutineSave(input());
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function save() {
    busy = true;
    error = null;
    try {
      const label = await api.saveRoutine(input());
      app.notify(routine ? `Routine ${routine.name} modifiée.` : `Routine ${name.trim()} créée et chargée.`);
      ondone(label);
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
  <div class="dialog" role="dialog" aria-labelledby="re-title">
    <h2 id="re-title">{routine ? `Modifier la routine ${routine.name}` : 'Nouvelle routine'}</h2>

    {#if !preview}
      <label>
        <span>Nom</span>
        {#if routine}
          <span class="mono fixed">{routine.label}</span>
        {:else}
          <input class="field mono" bind:this={nameInput} bind:value={name} placeholder="rapport-du-soir" spellcheck="false" autocomplete="off" />
        {/if}
      </label>
      {#if !routine}
        <p class="hint muted">Minuscules, chiffres et tirets. Label : <code>{app.routinePrefix}{name.trim() || '…'}</code></p>
      {/if}
      <label>
        <span>Programme</span>
        <input class="field mono" bind:this={programInput} bind:value={program} placeholder="/usr/bin/python3" spellcheck="false" autocomplete="off" />
      </label>
      <p class="hint muted">Chemin absolu. Aucun shell n'interprète les arguments : pour un pipe ou une redirection, <code>/bin/sh</code> puis <code>-c</code>.</p>
      <div class="label">
        <span>Arguments</span>
        <div class="args">
          {#each args as _, i (i)}
            <div class="arg">
              <textarea class="field mono" rows="1" bind:value={args[i]} spellcheck="false" aria-label="Argument {i + 1}"></textarea>
              <button class="btn ghost" onclick={() => args.splice(i, 1)} disabled={args.length === 1} aria-label="Retirer l'argument {i + 1}" title="Retirer">✕</button>
            </div>
          {/each}
          <button class="btn small add" onclick={() => args.push('')} disabled={args.length >= 63}>Ajouter un argument</button>
        </div>
      </div>
      <label>
        <span>Dossier de travail</span>
        <input class="field mono" bind:value={workingDir} placeholder="optionnel" spellcheck="false" autocomplete="off" />
      </label>
      <div class="label">
        <span>Jours</span>
        <div class="days">
          {#each DAYS as d, i (d)}
            <button class="day" aria-pressed={days.includes(i + 1)} onclick={() => toggleDay(i + 1)}>{d}</button>
          {/each}
          <button class="btn ghost small" onclick={() => (days = [1, 2, 3, 4, 5])}>Jours ouvrés</button>
          <button class="btn ghost small" onclick={() => (days = [1, 2, 3, 4, 5, 6, 7])}>Tous</button>
        </div>
      </div>
      <label>
        <span>Heures</span>
        <input class="field mono" bind:value={times} placeholder="9:00, 17:30" spellcheck="false" autocomplete="off" />
      </label>
      {#if !routine}
        <p class="hint muted">
          Journaux sous <code>~/Library/Logs/Robiade/routines</code>. Le <code>PATH</code> de ton shell est
          copié dans le plist : launchd n'a sinon que <code>/usr/bin:/bin:/usr/sbin:/sbin</code>.
        </p>
      {:else}
        <p class="hint muted">Les journaux et le <code>PATH</code> de la routine restent ceux du plist.</p>
      {/if}
    {:else}
      <div class="grid">
        <span class="k">Fichier</span>
        <span class="mono path">{shortPath(preview.file, home)}</span>
        <span class="k">Planning</span>
        <span class="path">{[preview.schedule, preview.nextRun ? `prochain passage ${dateTime(preview.nextRun)}` : ''].filter(Boolean).join(' · ')}</span>
      </div>
      <pre class="mono plist">{preview.plist}</pre>
      <p class="note muted">
        {routine
          ? 'launchd décharge la routine, le plist est réécrit, puis rechargé (sauf si elle est désactivée). Refusé, l’ancien revient.'
          : 'Le plist est écrit puis chargé par launchd. Refusé, il est retiré.'}
      </p>
      {#if preview.blocker}<p class="error">{preview.blocker}</p>{/if}
    {/if}

    {#if error}<p class="error">{error}</p>{/if}
    <footer>
      <button class="btn" onclick={onclose}>Annuler</button>
      {#if preview}
        <button class="btn" onclick={() => (preview = null)} disabled={busy}>Retour</button>
        <button class="btn primary" disabled={!!preview.blocker || busy} onclick={() => void save()}>
          {busy ? 'Enregistrement…' : routine ? 'Réécrire et recharger' : 'Créer et charger'}
        </button>
      {:else}
        <button class="btn primary" disabled={busy || program.trim() === '' || (!routine && name.trim() === '')} onclick={() => void showPreview()}>
          {busy ? 'Vérification…' : 'Voir le plist'}
        </button>
      {/if}
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 80; display: grid; justify-items: center; align-items: start;
    padding-top: 8vh; background: rgb(0 0 0 / 0.4); backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(680px, 94vw); max-height: 86vh; overflow-y: auto; background: var(--bg-raised); border-radius: 12px;
    box-shadow: var(--shadow); padding: 18px; display: flex; flex-direction: column; gap: 10px;
  }
  h2 { margin: 0 0 4px; font-size: 15px; overflow-wrap: anywhere; }
  p { margin: 0; }
  label, .label { display: grid; grid-template-columns: 140px 1fr; align-items: start; gap: 12px; }
  label > span:first-child, .label > span:first-child { color: var(--ink-2); font-size: 13px; padding-top: 5px; }
  .fixed { padding-top: 5px; font-size: 12.5px; overflow-wrap: anywhere; }
  .hint { font-size: 12px; margin-left: 152px; }
  .note { font-size: 12px; }
  code { font-family: var(--mono); font-size: 11.5px; overflow-wrap: anywhere; }
  .args { display: flex; flex-direction: column; gap: 4px; }
  .arg { display: flex; gap: 4px; align-items: start; }
  .arg textarea { flex: 1; resize: vertical; min-height: 28px; field-sizing: content; max-height: 160px; }
  .arg .btn { width: 28px; padding: 0; justify-content: center; }
  .add { align-self: flex-start; }
  .days { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; }
  .day {
    height: 26px; min-width: 40px; padding: 0 8px; border-radius: var(--radius-sm); border: 1px solid var(--line-strong);
    background: none; color: var(--ink-2); font-size: 12.5px;
  }
  .day[aria-pressed='true'] { background: var(--accent-soft); border-color: var(--accent); color: var(--ink); font-weight: 550; }
  .btn.small { height: 24px; padding: 0 8px; font-size: 12px; }
  .grid { display: grid; grid-template-columns: 80px 1fr; gap: 6px 12px; align-items: baseline; }
  .k { color: var(--ink-3); font-size: 12.5px; }
  .path { font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; }
  .plist {
    margin: 0; padding: 10px 12px; border-radius: var(--radius-sm); background: var(--bg-hover);
    font-size: 11.5px; line-height: 1.45; white-space: pre-wrap; overflow-wrap: anywhere; max-height: 46vh; overflow-y: auto;
  }
  .error { color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
  .btn:disabled { opacity: 0.5; }
</style>
