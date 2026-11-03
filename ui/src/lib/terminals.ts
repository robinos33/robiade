import { Terminal, type IBuffer, type ILink, type ILinkProvider } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { api } from './api';
import { LINK_PATTERN, openLink } from './links';

/**
 * Un terminal xterm par agent, conservé hors du DOM quand on change de vue :
 * l'historique d'affichage survit à la navigation.
 */
interface Host {
  term: Terminal;
  fit: FitAddon;
  el: HTMLDivElement;
  opened: boolean;
  /** Ouvre un lien `file://` cliqué dans ce terminal ; fourni par la vue qui l'affiche. */
  onfile?: (uri: string) => void;
}

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

/**
 * ⌘ enfoncé : un lien survolé est souligné et le curseur devient une main, comme dans iTerm. Sans
 * ⌘, un clic reste un clic dans le terminal (sélection, interactions de Claude Code).
 */
let meta = false;
/** Liens sous la souris ; xterm suit leurs `decorations` et redessine quand elles changent. */
const hovered = new Set<ILink>();

function setMeta(held: boolean) {
  if (held === meta) return;
  meta = held;
  for (const link of hovered) Object.assign(link.decorations ?? {}, { underline: held, pointerCursor: held });
}

window.addEventListener('keydown', (e) => setMeta(e.metaKey), true);
window.addEventListener('keyup', (e) => setMeta(e.metaKey), true);
// ⌘⇥ vers une autre app : le relâchement de ⌘ ne nous parvient pas.
window.addEventListener('blur', () => setMeta(false));

/**
 * Dernière sélection faite dans l'interface, terminal ou DOM, comme la sélection primaire de X11 :
 * le clic molette dans un terminal d'agent la colle dans son prompt. On garde le texte, pas la
 * sélection : il survit à la désélection et au changement de vue, ce qui permet de reprendre un
 * titre d'issue ou un bout de diff affichés ailleurs (zones `.selectable`, le reste de l'interface
 * est en `user-select: none`). Le presse-papiers n'est pas touché.
 */
let primary = '';

document.addEventListener('selectionchange', () => {
  const text = document.getSelection()?.toString() ?? '';
  if (text) primary = text;
});

/**
 * Caractères de contrôle C0 et C1, sauf tabulation et fins de ligne. Un titre d'issue ou un diff
 * vient de GitLab : un `ESC[201~` fermerait le bracketed paste et la suite partirait comme tapée.
 */
const CONTROL = /[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f]/g;

/**
 * Colle la dernière sélection dans le prompt, sans l'envoyer. `paste` passe en bracketed paste et
 * normalise les fins de ligne, mais laisse passer les séquences d'échappement : on les retire. Une
 * fin de ligne finale (triple-clic sur une ligne) est retirée aussi.
 */
function pastePrimary(term: Terminal) {
  const text = primary.replace(CONTROL, '').replace(/[\r\n]+$/, '');
  if (!text) return;
  // Sans bracketed paste, chaque `\r` vaudrait Entrée. Claude Code l'active toujours.
  if (!term.modes.bracketedPasteMode && /[\r\n]/.test(text)) return;
  term.paste(text);
  term.focus();
}

/** Lignes repliées recollées au plus, de part et d'autre de la ligne survolée. */
const MAX_WRAPPED_ROWS = 32;

/**
 * Ligne logique autour de la ligne `y` du buffer (à partir de 0) : les lignes que le terminal a
 * repliées y sont recollées, pour qu'une URL plus longue que la largeur reste un seul lien.
 * `cells[i]` est la cellule `[x, y]` d'où vient `text[i]` : un caractère large en occupe deux.
 */
function logicalLine(buffer: IBuffer, y: number): { text: string; cells: [number, number][] } {
  let top = y;
  while (top > 0 && y - top < MAX_WRAPPED_ROWS && buffer.getLine(top)?.isWrapped) top--;
  let text = '';
  const cells: [number, number][] = [];
  const cell = buffer.getNullCell();
  for (let row = top; row <= y + MAX_WRAPPED_ROWS; row++) {
    const line = buffer.getLine(row);
    if (!line || (row > top && !line.isWrapped)) break;
    for (let x = 0; x < line.length; x++) {
      if (!line.getCell(x, cell)?.getWidth()) continue; // seconde moitié d'un caractère large
      const chars = cell.getChars() || ' ';
      text += chars;
      for (let i = 0; i < chars.length; i++) cells.push([x, row]);
    }
  }
  return { text, cells };
}

/** URL affichées en clair, web ou `file://`. */
function plainLinks(term: Terminal, activate: (event: MouseEvent, uri: string) => void): ILinkProvider {
  return {
    provideLinks(y, callback) {
      const { text, cells } = logicalLine(term.buffer.active, y - 1);
      const links = [...text.matchAll(LINK_PATTERN)].flatMap((m): ILink[] => {
        const start = cells[m.index];
        const end = cells[m.index + m[0].length - 1];
        if (!start || !end) return [];
        const link: ILink = {
          range: { start: { x: start[0] + 1, y: start[1] + 1 }, end: { x: end[0] + 1, y: end[1] + 1 } },
          text: m[0],
          // Lues par xterm à l'arrivée sur le lien ; `setMeta` les change ensuite.
          decorations: {
            get underline() { return meta; },
            get pointerCursor() { return meta; },
          },
          activate,
          hover: () => void hovered.add(link),
          leave: () => void hovered.delete(link),
        };
        return [link];
      });
      callback(links.length ? links : undefined);
    },
  };
}

function host(id: string): Host {
  let h = hosts.get(id);
  if (h) return h;
  const activate = (event: MouseEvent, uri: string) => {
    if (event.metaKey) openLink(uri, (u) => hosts.get(id)?.onfile?.(u));
  };
  const term = new Terminal({
    fontFamily: cssVar('--mono'),
    fontSize: 13,
    lineHeight: 1.15,
    cursorBlink: true,
    scrollback: 10000,
    allowProposedApi: false,
    macOptionIsMeta: true,
    theme: TERMINAL_THEME,
    // Liens OSC 8 : Claude Code n'en émet pas dans notre PTY (`FORCE_HYPERLINK=0`), mais un autre
    // programme le pourrait. Sans ce gestionnaire, xterm ouvrirait `confirm()` puis `window.open`.
    // Les autres schémas sont reçus pour être refusés par `openLink`, sans boîte de dialogue.
    linkHandler: { activate, allowNonHttpProtocols: true },
  });
  term.registerLinkProvider(plainLinks(term, activate));
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.onData((data) => void api.writeAgent(id, data).catch(() => {}));
  term.onResize(({ cols, rows }) => void api.resizeAgent(id, cols, rows).catch(() => {}));
  // Le texte est lu pendant le glisser : un clic ultérieur dans le terminal efface la surbrillance,
  // pas la dernière sélection.
  term.onSelectionChange(() => {
    if (term.hasSelection()) primary = term.getSelection();
  });
  const el = document.createElement('div');
  el.className = 'term-host';
  // ⌘ enfoncé avant que la fenêtre ait le focus : le premier mouvement de souris le dit.
  el.addEventListener('mousemove', (e) => setMeta(e.metaKey));
  // Clic molette, intercepté avant xterm : si le programme suit la souris, xterm lui transmettrait
  // le clic. `preventDefault` empêche aussi le défilement automatique et l'ouverture d'un lien.
  const middle = (e: MouseEvent) => {
    if (e.button !== 1) return false;
    e.preventDefault();
    e.stopPropagation();
    return true;
  };
  el.addEventListener('mousedown', (e) => void (middle(e) && pastePrimary(term)), true);
  el.addEventListener('mouseup', middle, true);
  el.addEventListener('auxclick', middle, true);
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

export function attach(id: string, container: HTMLElement, onfile: (uri: string) => void) {
  const h = host(id);
  h.onfile = onfile;
  container.replaceChildren(h.el);
  const fresh = !h.opened;
  if (fresh) {
    h.term.open(h.el);
    h.opened = true;
  }
  refit(id);
  if (fresh) void redraw(id, h.term);
  h.term.focus();
}

/**
 * Après un rechargement de l'interface, l'agent tourne toujours mais son xterm est neuf et vide,
 * alors que Claude Code ne réécrit que les cellules qui changent. Une colonne de moins puis la
 * taille réelle : deux SIGWINCH, et Claude Code redessine tout l'écran.
 */
async function redraw(id: string, term: Terminal) {
  try {
    await api.resizeAgent(id, Math.max(term.cols - 1, 1), term.rows);
    await api.resizeAgent(id, term.cols, term.rows);
  } catch {
    // Agent terminé entre-temps : rien à redessiner.
  }
}

export function refit(id: string) {
  const h = hosts.get(id);
  if (h?.opened && h.el.isConnected) h.fit.fit();
}

export function dispose(id: string) {
  hosts.get(id)?.term.dispose();
  hosts.delete(id);
}
