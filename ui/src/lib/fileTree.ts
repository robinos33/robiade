/** Arbre de fichiers de l'onglet Code, construit depuis la liste à plat que renvoie git. */

/** Contenu direct d'un dossier, en chemins complets, dossiers puis fichiers dans l'ordre d'affichage. */
export interface Dir {
  dirs: string[];
  files: string[];
}

/** Une ligne affichée de l'arbre. */
export interface Row {
  path: string;
  name: string;
  depth: number;
  dir: boolean;
}

/** `src/10.rs` après `src/9.rs`, et sans tenir compte de la casse, comme le Finder. */
const collator = new Intl.Collator('fr', { numeric: true, sensitivity: 'base' });

export const baseName = (path: string) => path.slice(path.lastIndexOf('/') + 1);
/** Dossier parent ; `''` pour la racine. */
export const parentOf = (path: string) => path.slice(0, Math.max(path.lastIndexOf('/'), 0));

/** Dossiers de l'arbre, par chemin ; `''` est la racine. */
export function buildTree(files: string[]): Map<string, Dir> {
  const tree = new Map<string, Dir>([['', { dirs: [], files: [] }]]);
  const dirOf = (path: string): Dir => {
    let dir = tree.get(path);
    if (!dir) {
      dir = { dirs: [], files: [] };
      tree.set(path, dir);
      dirOf(parentOf(path)).dirs.push(path);
    }
    return dir;
  };
  for (const file of files) dirOf(parentOf(file)).files.push(file);
  const byName = (a: string, b: string) => collator.compare(baseName(a), baseName(b));
  for (const dir of tree.values()) {
    dir.dirs.sort(byName);
    dir.files.sort(byName);
  }
  return tree;
}

/** Lignes visibles : le contenu de la racine, et celui de chaque dossier déplié. */
export function visibleRows(tree: Map<string, Dir>, expanded: ReadonlySet<string>): Row[] {
  const rows: Row[] = [];
  const walk = (path: string, depth: number) => {
    const dir = tree.get(path);
    if (!dir) return;
    for (const d of dir.dirs) {
      rows.push({ path: d, name: baseName(d), depth, dir: true });
      if (expanded.has(d)) walk(d, depth + 1);
    }
    for (const f of dir.files) rows.push({ path: f, name: baseName(f), depth, dir: false });
  };
  walk('', 0);
  return rows;
}

/** Dossiers à déplier pour voir `path`, du plus haut au plus proche. */
export function ancestors(path: string): string[] {
  const found: string[] = [];
  for (let dir = parentOf(path); dir; dir = parentOf(dir)) found.unshift(dir);
  return found;
}
