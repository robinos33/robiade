/**
 * Ordre des projets rangés à la main : une liste de racines de dépôt, propre au poste. Une racine
 * absente n'a jamais été rangée ; une racine dont le projet a disparu garde sa place, qu'il
 * retrouve s'il revient.
 */
const KEY = 'robiade.projects.order';

export function storedOrder(): string[] {
  try {
    const roots: unknown = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    return Array.isArray(roots) ? roots.filter((r): r is string => typeof r === 'string') : [];
  } catch {
    return [];
  }
}

export function storeOrder(order: string[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(order));
  } catch {
    // Stockage inaccessible : l'ordre vaut pour la session.
  }
}

/**
 * Les projets qui ont un agent vivant d'abord, puis les autres ; dans chaque groupe, l'ordre rangé,
 * et après lui les projets jamais rangés, dans l'ordre reçu. Le tri est stable.
 */
export function orderProjects<P extends { root: string }>(projects: P[], order: string[], live: (root: string) => boolean): P[] {
  const rank = new Map(order.map((root, i) => [root, i]));
  const key = (p: P): [number, number] => [live(p.root) ? 0 : 1, rank.get(p.root) ?? Infinity];
  return [...projects].sort((a, b) => {
    const [ga, ra] = key(a);
    const [gb, rb] = key(b);
    return ga - gb || (ra === rb ? 0 : ra < rb ? -1 : 1);
  });
}

/**
 * Nouvel ordre après le dépôt de `moved` juste avant `before`, ou après `after` en fin de groupe.
 * Les projets affichés et jamais rangés le deviennent d'abord, à leur place affichée, pour que le
 * dépôt ait un repère : l'affichage n'en change pas. Les autres racines gardent leur rang relatif.
 */
export function moveInOrder(
  order: string[],
  shown: string[],
  moved: string,
  target: { before: string } | { after: string },
): string[] {
  const known = new Set(order);
  const next = [...order, ...shown.filter((r) => !known.has(r))].filter((r) => r !== moved);
  const at = 'before' in target ? next.indexOf(target.before) : next.indexOf(target.after) + 1;
  if (at < 0 || ('after' in target && at === 0)) return order;
  next.splice(at, 0, moved);
  return next;
}
