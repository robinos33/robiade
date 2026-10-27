import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { api } from './api';

/**
 * Un terminal xterm par agent, conservé hors du DOM quand on change de vue :
 * l'historique d'affichage survit à la navigation.
 */
interface Host { term: Terminal; fit: FitAddon; el: HTMLDivElement; opened: boolean }

const hosts = new Map<string, Host>();

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function theme() {
  return {
    background: cssVar('--bg-sunken'),
    foreground: cssVar('--ink'),
    cursor: cssVar('--accent'),
    cursorAccent: cssVar('--bg-sunken'),
    selectionBackground: cssVar('--accent-soft'),
  };
}

function host(id: string): Host {
  let h = hosts.get(id);
  if (h) return h;
  const term = new Terminal({
    fontFamily: cssVar('--mono'),
    fontSize: 13,
    lineHeight: 1.15,
    cursorBlink: true,
    scrollback: 10000,
    allowProposedApi: false,
    macOptionIsMeta: true,
    theme: theme(),
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.onData((data) => void api.writeAgent(id, data).catch(() => {}));
  term.onResize(({ cols, rows }) => void api.resizeAgent(id, cols, rows).catch(() => {}));
  const el = document.createElement('div');
  el.className = 'term-host';
  h = { term, fit, el, opened: false };
  hosts.set(id, h);
  return h;
}

export function writeOutput(id: string, base64: string) {
  const bin = atob(base64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  host(id).term.write(bytes);
}

export function attach(id: string, container: HTMLElement) {
  const h = host(id);
  container.replaceChildren(h.el);
  if (!h.opened) {
    h.term.open(h.el);
    h.opened = true;
  }
  refit(id);
  h.term.focus();
}

export function refit(id: string) {
  const h = hosts.get(id);
  if (h?.opened && h.el.isConnected) h.fit.fit();
}

export function dispose(id: string) {
  hosts.get(id)?.term.dispose();
  hosts.delete(id);
}
