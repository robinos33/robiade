import { openUrl } from '@tauri-apps/plugin-opener';
import { api } from './api';
import { fileSize } from './format';
import type { AssetContent } from './types';

/**
 * URL web ou de fichier local affichée en clair. Bornes reprises de `@xterm/addon-web-links` :
 * espaces, guillemets et chevrons l'arrêtent ; ponctuation finale et parenthèses qui l'entourent
 * n'en font pas partie.
 */
export const LINK_PATTERN = /(?:https?|file):\/\/[^\s"'!*(){}|\\^<>`]*[^\s"':,.!?{}|\\^~[\]`()<>]/gi;

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
  if (url.protocol === 'http:' || url.protocol === 'https:') void openUrl(url.href);
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
  const file = await api.readLinkedFile(agentId, uri);
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
