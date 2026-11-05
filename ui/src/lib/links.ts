import { api } from './api';
import { fileSize } from './format';
import type { AssetContent, ProjectFile } from './types';

/**
 * URL web ou de fichier local affichée en clair. Bornes reprises de `@xterm/addon-web-links` :
 * espaces, guillemets et chevrons l'arrêtent ; ponctuation finale et parenthèses qui l'entourent
 * n'en font pas partie.
 */
export const LINK_PATTERN = /(?:https?|file):\/\/[^\s"'!*(){}|\\^<>`]*[^\s"':,.!?{}|\\^~[\]`()<>]/gi;

/** Chemin de fichier cité en clair dans un texte (`src/foo.rs:42:7`), et sa place dans ce texte. */
export interface CitedPath {
  index: number;
  /** Longueur du texte cité, ligne et colonne comprises. */
  length: number;
  path: string;
  line?: number;
}

/**
 * Suite de caractères de chemin, suivie de `:ligne` ou `:ligne:colonne`. Elle ne finit pas par un
 * point : celui qui clôt une phrase n'en fait pas partie.
 */
const CITED_PATH = /(?<![\p{L}\p{N}_.@+~/-])[\p{L}\p{N}_.@+~/-]*[\p{L}\p{N}_@+~/-](?::(\d+)(?::\d+)?)?/gu;

/** Un chemin a un dossier (`src/a`) ou une extension faite de lettres (`a.rs`, pas `1.5`). */
const LOOKS_LIKE_PATH = /^(?=.*[\p{L}\p{N}]).*\/|\.\p{L}[\p{L}\p{N}]*$/u;

/**
 * Chemins que Claude Code cite sans `file://` : relatifs à son dossier (`src/foo.rs:42`), absolus
 * ou sous `~/`. Seul l'aspect compte ici, quitte à prendre `et/ou` : Rust dit ensuite lesquels
 * désignent un fichier. Ce qui chevauche une URL de `urls` lui appartient.
 */
export function citedPaths(text: string, urls: { index: number; 0: string }[]): CitedPath[] {
  return [...text.matchAll(CITED_PATH)].flatMap((m): CitedPath[] => {
    const path = m[1] === undefined ? m[0] : m[0].slice(0, m[0].indexOf(':'));
    const end = m.index + m[0].length;
    const inUrl = urls.some((u) => m.index < u.index + u[0].length && u.index < end);
    if (inUrl || !LOOKS_LIKE_PATH.test(path)) return [];
    return [{ index: m.index, length: m[0].length, path, line: m[1] === undefined ? undefined : Number(m[1]) }];
  });
}

/** URL `file://` d'un chemin absolu, que `linkedPath` et Rust décodent à l'identique. */
export function fileUrl(path: string): string {
  return `file://${encodeURI(path).replace(/[?#]/g, encodeURIComponent)}`;
}

/**
 * Seule sortie d'un lien cliqué dans un terminal. La sortie d'un agent n'est pas fiable (page,
 * issue ou fichier relu) : le web part au navigateur, un `file://` au visualiseur de Robiade, qui
 * ne confie rien à une autre app ; tout autre schéma est ignoré, sans boîte de dialogue.
 */
export function openLink(uri: string, onfile: (uri: string) => void) {
  let url: URL;
  try {
    url = new URL(uri);
  } catch {
    return;
  }
  // Rust n'ouvre que les pages de la forge réglée ; un autre lien reste sans effet, comme avant.
  if (url.protocol === 'http:' || url.protocol === 'https:') api.openForgeUrl(url.href).catch(() => {});
  else if (url.protocol === 'file:') onfile(uri);
}

/** Chemin affiché pour un lien `file://`. Rust le décode lui-même avant de lire quoi que ce soit. */
export function linkedPath(uri: string): string {
  try {
    return decodeURIComponent(new URL(uri).pathname);
  } catch {
    return uri;
  }
}

/** Fichier d'un lien `file://` ; un fichier non lu rejette avec la raison à afficher à sa place. */
export async function readLinkedFile(agentId: string, uri: string): Promise<AssetContent> {
  return viewable(await api.readLinkedFile(agentId, uri));
}

/** Contenu d'un fichier lu par Rust ; un fichier non lu lève la raison à afficher à sa place. */
export function viewable(file: ProjectFile): AssetContent {
  switch (file.state) {
    case 'content':
      return file;
    case 'secret':
      throw 'Fichier de secrets : non affiché.';
    case 'tooLarge':
      throw `Trop lourd pour l'aperçu : ${fileSize(file.size)}, pour une limite de ${fileSize(file.limit)}.`;
    case 'binary':
      throw `Fichier binaire (${fileSize(file.size)}) : pas d'aperçu.`;
  }
}
