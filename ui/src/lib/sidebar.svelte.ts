/**
 * Barre latérale masquée en entier, pour rendre sa largeur au contenu. Réglage d'affichage propre
 * au poste ; la clé n'existe que barre repliée, comme pour le repli des sections.
 */
const KEY = 'robiade.sidebar.collapsed';

function stored(): boolean {
  try {
    return localStorage.getItem(KEY) === '1';
  } catch {
    return false;
  }
}

export const sidebar = $state({ collapsed: stored() });

/** Le repli est instantané : les terminaux ne se recalent qu'une fois, au `ResizeObserver`. */
export function toggleSidebar(): void {
  sidebar.collapsed = !sidebar.collapsed;
  try {
    if (sidebar.collapsed) localStorage.setItem(KEY, '1');
    else localStorage.removeItem(KEY);
  } catch {
    // Stockage inaccessible : le repli vaut pour la session.
  }
}
