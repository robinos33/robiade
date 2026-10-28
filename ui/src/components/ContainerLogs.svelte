<script lang="ts">
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { listen } from '@tauri-apps/api/event';
  import { api } from '../lib/api';
  import { TERMINAL_THEME } from '../lib/terminals';

  let { id }: { id: string } = $props();
  // La liste est relue toutes les 5 s et `id` vient d'un objet neuf à chaque fois : sans ce
  // `$derived`, qui ne propage que si la chaîne change, le suivi repartirait de zéro.
  const containerId = $derived(id);

  let host: HTMLDivElement | undefined = $state();
  let ended = $state(false);
  let error = $state<string | null>(null);

  function decode(base64: string): Uint8Array {
    const bin = atob(base64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return bytes;
  }

  /**
   * Les logs passent par xterm, en lecture seule : les couleurs ANSI s'affichent, et rien de ce
   * qu'ils contiennent n'est interprété comme du HTML.
   */
  $effect(() => {
    const target = containerId;
    if (!host) return;
    ended = false;
    error = null;
    const term = new Terminal({
      fontFamily: getComputedStyle(document.documentElement).getPropertyValue('--mono').trim(),
      fontSize: 12,
      disableStdin: true,
      convertEol: true,
      scrollback: 5000,
      theme: TERMINAL_THEME,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    fit.fit();
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(host);

    let stream: number | null = null;
    let closed = false;
    // Les premiers morceaux peuvent arriver avant que l'identifiant du suivi soit connu.
    const early: { stream: number; data: string | null }[] = [];
    const write = (e: { stream: number; data: string | null }) => {
      if (e.stream !== stream) return;
      if (e.data === null) ended = true;
      else term.write(decode(e.data));
    };
    const unlisten = listen<{ stream: number; data: string | null }>('container:logs', (e) =>
      stream === null ? early.push(e.payload) : write(e.payload),
    );
    unlisten
      .then(() => api.followContainerLogs(target))
      .then((s) => {
        if (closed) return void api.stopContainerLogs(s);
        stream = s;
        early.splice(0).forEach(write);
      })
      .catch((e) => (error = String(e)));

    return () => {
      closed = true;
      if (stream !== null) void api.stopContainerLogs(stream);
      void unlisten.then((stop) => stop());
      observer.disconnect();
      term.dispose();
    };
  });
</script>

<div class="logs">
  <div class="term" bind:this={host}></div>
  {#if error}<p class="note error">{error}</p>
  {:else if ended}<p class="note muted">Fin du flux : le conteneur s'est arrêté.</p>{/if}
</div>

<style>
  .logs { display: flex; flex-direction: column; min-height: 0; flex: 1; background: #24273a; border-radius: var(--radius); overflow: hidden; }
  .term { flex: 1; min-height: 0; padding: 6px 0 0 8px; }
  .note { margin: 0; padding: 6px 10px; font-size: 12px; background: var(--bg-raised); }
  .error { color: var(--danger); }
</style>
