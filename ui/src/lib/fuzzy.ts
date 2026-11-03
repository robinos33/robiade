/**
 * Recherche de fichier par nom, façon ⌘P : les lettres tapées apparaissent dans l'ordre, pas
 * forcément d'affilée (`ppol` trouve `PromoCodePolicy.php`). Le nom du fichier passe avant le
 * reste du chemin ; puis les lettres collées, et celles qui ouvrent un mot.
 */

export interface Match {
  path: string;
  /** Indices, dans `path`, des caractères trouvés : de quoi les surligner. */
  positions: number[];
  score: number;
}

const SEPARATORS = '/._- ';

/** Début d'un mot : après un séparateur, ou une majuscule après une minuscule (`CodePolicy`). */
function opensWord(text: string, at: number): boolean {
  if (at === 0) return true;
  const before = text[at - 1]!;
  const here = text[at]!;
  return SEPARATORS.includes(before) || (before === before.toLowerCase() && here !== here.toLowerCase());
}

/**
 * Lettres de `query` (déjà en minuscules) dans `text`, à partir de `from`. Chaque lettre prend
 * de préférence un début de mot, sinon la première occurrence : assez bon, et linéaire.
 */
function locate(text: string, lower: string, query: string, from: number): { positions: number[]; score: number } | null {
  const positions: number[] = [];
  let score = 0;
  let at = from;
  for (const char of query) {
    const first = lower.indexOf(char, at);
    if (first < 0) return null;
    let found = first;
    // Prolonger la série en cours vaut mieux qu'un début de mot plus loin.
    if (positions.at(-1) !== first - 1) {
      for (let i = first; i >= 0; i = lower.indexOf(char, i + 1)) {
        if (opensWord(text, i)) {
          found = i;
          break;
        }
      }
    }
    if (positions.at(-1) === found - 1) score += 6;
    if (opensWord(text, found)) score += 4;
    score += 1;
    positions.push(found);
    at = found + 1;
  }
  return { positions, score };
}

/** Les `limit` meilleurs fichiers pour `query`, le meilleur en tête. */
export function findFiles(files: readonly string[], query: string, limit = 50): Match[] {
  const q = query.toLowerCase().replace(/\s+/g, '');
  if (!q) return [];
  const matches: Match[] = [];
  for (const path of files) {
    const lower = path.toLowerCase();
    const nameStart = path.lastIndexOf('/') + 1;
    // Tout dans le nom du fichier d'abord ; sinon, n'importe où dans le chemin.
    const inName = locate(path, lower, q, nameStart);
    const found = inName ? { ...inName, score: inName.score + 20 } : locate(path, lower, q, 0);
    if (!found) continue;
    // À score égal, le chemin le plus court : `src/Kernel.php` avant `tests/…/Kernel.php`.
    matches.push({ path, positions: found.positions, score: found.score - path.length / 100 });
  }
  matches.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path));
  return matches.slice(0, limit);
}
