import { Terminal, type IBuffer, type ILink, type ILinkProvider } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { SearchAddon, type ISearchOptions } from '@xterm/addon-search';
import { api } from './api';
import { citedPaths, fileUrl, LINK_PATTERN, openLink } from './links';

/**
 * Un terminal xterm par agent, conservé hors du DOM quand on change de vue :
 * l'historique d'affichage survit à la navigation.
 */
interface Host {
  term: Terminal;
  fit: FitAddon;
  search: SearchAddon;
  el: HTMLDivElement;
  opened: boolean;
  /**
   * Ouvre un lien `file://` cliqué dans ce terminal, à la ligne citée s'il y en a une ; fourni par
   * la vue qui l'affiche.
   */
  onfile?: OpenFile;
}

type OpenFile = (uri: string, line?: number) => void;

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

/** Lignes recollées au plus, de part et d'autre de la ligne survolée. */
const MAX_WRAPPED_ROWS = 32;

/** Caractère admis dans une URL, d'après `LINK_PATTERN` ; les chemins cités n'en ont pas d'autres. */
const URL_CHAR = /[^\s"'!*(){}|\\^<>`]/;

/**
 * Puce de liste ou de résultat d'outil (`⎿`) suivie d'une espace : une ligne qui commence ainsi
 * ouvre un nouvel élément, même après une URL arrêtée pile au bord.
 */
const BULLET = /^[-*+•⎿](?: |$)/;

/**
 * Si la ligne `row` du buffer continue la précédente, nombre de cellules à sauter à son début ;
 * -1 sinon. Le terminal marque repliée (`isWrapped`) une ligne qu'il a coupée lui-même. Claude
 * Code (Ink), lui, coupe à la largeur avec un vrai saut de ligne, puis indente la suite (2
 * colonnes, 4 dans une liste) : une URL coupée remplit alors la ligne jusqu'à la dernière colonne,
 * et sa suite reprend juste après l'indentation, qui ne fait pas partie du lien. Une ligne arrêtée
 * au bord par hasard se colle aussi à la suivante : sans effet tant qu'une URL ou un chemin ne
 * finit pas au bord.
 */
function continuation(buffer: IBuffer, cols: number, row: number): number {
  const line = buffer.getLine(row);
  if (!line || row === 0) return -1;
  if (line.isWrapped) return 0;
  const edge = buffer.getLine(row - 1)?.getCell(cols - 1)?.getChars() ?? '';
  if (!URL_CHAR.test(edge)) return -1;
  const text = line.translateToString(false, 0, cols);
  const rest = text.replace(/^ +/, '');
  return URL_CHAR.test(rest.charAt(0)) && !BULLET.test(rest) ? text.length - rest.length : -1;
}

/**
 * Ligne logique autour de la ligne `y` du buffer (à partir de 0) : les lignes qui continuent la
 * précédente y sont recollées, pour qu'une URL plus longue que la largeur reste un seul lien.
 * `cells[i]` est la cellule `[x, y]` d'où vient `text[i]` : un caractère large en occupe deux.
 * Seules les `cols` premières cellules comptent : après un rétrécissement, xterm garde des lignes
 * plus longues, dont la fin invisible couperait l'URL par des blancs.
 */
function logicalLine(buffer: IBuffer, cols: number, y: number): { text: string; cells: [number, number][] } {
  let top = y;
  while (top > 0 && y - top < MAX_WRAPPED_ROWS && continuation(buffer, cols, top) >= 0) top--;
  let text = '';
  const cells: [number, number][] = [];
  const cell = buffer.getNullCell();
  for (let row = top; row <= y + MAX_WRAPPED_ROWS; row++) {
    const line = buffer.getLine(row);
    const skip = row === top ? 0 : continuation(buffer, cols, row);
    if (!line || skip < 0) break;
    for (let x = skip; x < Math.min(line.length, cols); x++) {
      if (!line.getCell(x, cell)?.getWidth()) continue; // seconde moitié d'un caractère large
      const chars = cell.getChars() || ' ';
      text += chars;
      for (let i = 0; i < chars.length; i++) cells.push([x, row]);
    }
  }
  return { text, cells };
}

/**
 * Liens d'une ligne : URL affichées en clair, web ou `file://`, et chemins de fichier cités sans
 * `file://` (`src/foo.rs:42`). Un chemin n'est un lien que si Rust dit qu'il désigne un fichier
 * lisible : la question ne part qu'au survol d'une ligne, et xterm garde la réponse tant que la
 * souris y reste.
 */
function lineLinks(term: Terminal, id: string, activate: (event: MouseEvent, uri: string) => void): ILinkProvider {
  return {
    provideLinks(y, callback) {
      const { text, cells } = logicalLine(term.buffer.active, term.cols, y - 1);
      const link = (index: number, length: number, open: (event: MouseEvent, text: string) => void): ILink[] => {
        const start = cells[index];
        const end = cells[index + length - 1];
        if (!start || !end) return [];
        const link: ILink = {
          range: { start: { x: start[0] + 1, y: start[1] + 1 }, end: { x: end[0] + 1, y: end[1] + 1 } },
          text: text.slice(index, index + length),
          // Lues par xterm à l'arrivée sur le lien ; `setMeta` les change ensuite.
          decorations: {
            get underline() { return meta; },
            get pointerCursor() { return meta; },
          },
          activate: open,
          hover: () => void hovered.add(link),
          leave: () => void hovered.delete(link),
        };
        return [link];
      };
      const urls = [...text.matchAll(LINK_PATTERN)];
      const found = urls.flatMap((m) => link(m.index, m[0].length, activate));
      const cited = citedPaths(text, urls);
      const done = (links: ILink[]) => callback(links.length ? links : undefined);
      if (!cited.length) return done(found);
      api.resolveLinkedPaths(id, cited.map((c) => c.path)).then(
        (resolved) => done([
          ...found,
          ...cited.flatMap(({ index, length, line }, i) => {
            const path = resolved[i];
            if (!path) return [];
            return link(index, length, (event) => {
              if (event.metaKey) hosts.get(id)?.onfile?.(fileUrl(path), line);
            });
          }),
        ]),
        () => done(found),
      );
    },
  };
}

/**
 * Raccourci de Robiade tapé dans un terminal. xterm laisse passer ses trois événements (keydown,
 * keypress, keyup) sans les traiter, la vue le reçoit en keydown : rien ne part dans le PTY.
 */
export type TerminalShortcut = 'find' | 'bigger' | 'smaller' | 'reset' | 'full';

export function terminalShortcut(e: KeyboardEvent): TerminalShortcut | null {
  if (!e.metaKey || e.ctrlKey || e.altKey) return null;
  if (e.key === 'f') return 'find';
  // ⌘⇧↩, comme iTerm pour agrandir un panneau.
  if (e.key === 'Enter' && e.shiftKey) return 'full';
  if (e.key === '+' || e.key === '=') return 'bigger';
  if (e.key === '-') return 'smaller';
  // En AZERTY, 0 se tape avec ⇧ : la touche compte, pas le caractère.
  if (e.key === '0' || e.code === 'Digit0') return 'reset';
  return null;
}

const FONT_SIZE_KEY = 'robiade.terminal.fontSize';
const FONT_SIZE = { min: 9, max: 24, initial: 13 };

/** Taille de police des terminaux d'agent, réglée sur ce poste. */
let fontSize = storedFontSize();

function storedFontSize(): number {
  try {
    const stored = Number(localStorage.getItem(FONT_SIZE_KEY));
    return Number.isInteger(stored) && stored >= FONT_SIZE.min && stored <= FONT_SIZE.max ? stored : FONT_SIZE.initial;
  } catch {
    return FONT_SIZE.initial;
  }
}

/**
 * ⌘+, ⌘− et ⌘0 : un point de plus ou de moins, ou la taille d'origine, pour tous les terminaux
 * d'agent. Ceux qui sont affichés se recalent tout de suite, et le PTY reçoit leur nouvelle
 * taille ; les autres, quand `attach` les réaffiche.
 */
export function resizeFont(step: 'bigger' | 'smaller' | 'reset') {
  const wanted = step === 'reset' ? FONT_SIZE.initial : fontSize + (step === 'bigger' ? 1 : -1);
  const next = Math.min(Math.max(wanted, FONT_SIZE.min), FONT_SIZE.max);
  if (next === fontSize) return;
  fontSize = next;
  try {
    localStorage.setItem(FONT_SIZE_KEY, String(next));
  } catch {
    // Stockage inaccessible : la taille vaut pour la session.
  }
  for (const [id, h] of hosts) {
    h.term.options.fontSize = next;
    refit(id);
  }
}

/** Occurrences en surbrillance, dans les couleurs du thème (xterm exige du `#RRGGBB`). */
const FIND_OPTIONS: ISearchOptions = {
  decorations: {
    matchBackground: '#494d64',
    matchBorder: '#8aadf4',
    matchOverviewRuler: '#8aadf4',
    activeMatchBackground: '#7a6a45',
    activeMatchBorder: '#eed49f',
    activeMatchColorOverviewRuler: '#eed49f',
  },
};

/**
 * Cherche `text` dans l'historique du terminal : `typing` étend l'occurrence courante pendant la
 * frappe, `next` et `previous` passent à la suivante ou à la précédente.
 */
export function find(id: string, text: string, move: 'typing' | 'next' | 'previous') {
  const search = hosts.get(id)?.search;
  if (!search) return;
  if (!text) return search.clearDecorations();
  if (move === 'previous') search.findPrevious(text, FIND_OPTIONS);
  else search.findNext(text, { ...FIND_OPTIONS, incremental: move === 'typing' });
}

/** Position de l'occurrence courante (-1 au-delà de 1 000 occurrences) et nombre d'occurrences. */
export function onFindResults(id: string, listener: (index: number, count: number) => void): () => void {
  const subscription = hosts.get(id)?.search.onDidChangeResults((r) => listener(r.resultIndex, r.resultCount));
  return () => subscription?.dispose();
}

/** Fin de recherche : surbrillances effacées, le clavier revient au terminal. */
export function endFind(id: string, focus: boolean) {
  const h = hosts.get(id);
  h?.search.clearDecorations();
  if (focus) h?.term.focus();
}

function host(id: string): Host {
  let h = hosts.get(id);
  if (h) return h;
  const activate = (event: MouseEvent, uri: string) => {
    if (event.metaKey) openLink(uri, (u) => hosts.get(id)?.onfile?.(u));
  };
  const term = new Terminal({
    fontFamily: cssVar('--mono'),
    fontSize,
    lineHeight: 1.15,
    cursorBlink: true,
    scrollback: 10000,
    // Les surbrillances de la recherche (`registerDecoration`) font encore partie de l'API proposée.
    allowProposedApi: true,
    macOptionIsMeta: true,
    theme: TERMINAL_THEME,
    // Liens OSC 8 : Claude Code n'en émet pas dans notre PTY (`FORCE_HYPERLINK=0`), mais un autre
    // programme le pourrait. Sans ce gestionnaire, xterm ouvrirait `confirm()` puis `window.open`.
    // Les autres schémas sont reçus pour être refusés par `openLink`, sans boîte de dialogue.
    linkHandler: { activate, allowNonHttpProtocols: true },
  });
  term.registerLinkProvider(lineLinks(term, id, activate));
  const fit = new FitAddon();
  term.loadAddon(fit);
  const search = new SearchAddon();
  term.loadAddon(search);
  term.attachCustomKeyEventHandler((e) => !terminalShortcut(e));
  term.onData((data) => void api.writeAgent(id, data).catch(() => {}));
  term.onResize(({ cols, rows }) => void api.resizeAgent(id, cols, rows).catch(() => {}));
  // Le texte est lu pendant le glisser : un clic ultérieur dans le terminal efface la surbrillance,
  // pas la dernière sélection.
  term.onSelectionChange(() => {
    if (term.hasSelection()) primary = term.getSelection();
  });
  const el = document.createElement('div');
  el.className = 'term-host';
  el.dataset.agent = id;
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
  h = { term, fit, search, el, opened: false };
  hosts.set(id, h);
  return h;
}

/** Agent dont le terminal est sous ce point de la fenêtre, en pixels CSS. */
export function terminalAt(x: number, y: number): string | undefined {
  return document.elementFromPoint(x, y)?.closest<HTMLElement>('.term-host')?.dataset.agent;
}

/**
 * Chemin échappé pour le shell comme le fait iTerm, par des antislashs : Claude Code reconnaît
 * ainsi une image déposée. Seuls les blancs et les caractères spéciaux du shell sont échappés.
 */
function shellEscape(path: string): string {
  return path.replace(/[\s\\'"`$&;|*?[\]{}()<>#~!^]/g, '\\$&');
}

/** Un caractère de contrôle C0 ou C1, retour à la ligne et tabulation compris. */
const ANY_CONTROL = /[\x00-\x1f\x7f-\x9f]/;

/**
 * Fichiers déposés sur le terminal : leurs chemins vont au prompt, séparés et suivis d'une espace
 * pour enchaîner un autre dépôt, sans Entrée. Collés et non tapés : Claude Code reçoit un collage.
 * Un nom de fichier peut contenir un retour à la ligne ou un `ESC`, que l'antislash ne neutralise
 * pas : il validerait le prompt ou sortirait du collage. Le dépôt est alors refusé, en entier ;
 * renvoie `false`.
 */
export function dropPaths(id: string, paths: string[]): boolean {
  const h = hosts.get(id);
  if (!h || !paths.length) return true;
  if (paths.some((p) => ANY_CONTROL.test(p))) return false;
  h.term.paste(`${paths.map(shellEscape).join(' ')} `);
  h.term.focus();
  return true;
}

export function writeOutput(id: string, base64: string) {
  const bin = atob(base64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  host(id).term.write(bytes);
}

export function attach(id: string, container: HTMLElement, onfile: OpenFile) {
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
