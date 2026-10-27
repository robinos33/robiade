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

/**
 * Catppuccin Macchiato, repris à l'identique du profil iTerm (mêmes couleurs en mode clair et sombre) :
 * Claude Code s'affiche comme dans mon terminal habituel.
 */
export const TERMINAL_THEME = {
  background: '#24273a',
  foreground: '#cad3f5',
  cursor: '#f4dbd6',
  cursorAccent: '#24273a',
  selectionBackground: '#5b6078',
  selectionForeground: '#cad3f5',
  black: '#494d64',
  red: '#ed8796',
  green: '#a6da95',
  yellow: '#eed49f',
  blue: '#8aadf4',
  magenta: '#f5bde6',
  cyan: '#8bd5ca',
  white: '#a5adcb',
  brightBlack: '#5b6078',
  brightRed: '#ec7486',
  brightGreen: '#8ccf7f',
  brightYellow: '#e1c682',
  brightBlue: '#78a1f6',
  brightMagenta: '#f2a9dd',
  brightCyan: '#63cbc0',
  brightWhite: '#b8c0e0',
};

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
    theme: TERMINAL_THEME,
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
