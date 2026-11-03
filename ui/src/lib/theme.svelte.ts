/**
 * Thème de l'interface. « Système » suit macOS, à chaud ; « Clair » et « Sombre » le forcent.
 * Réglage d'affichage propre au poste, rangé dans localStorage comme les brouillons de la console DB.
 */
export type ThemeChoice = 'system' | 'light' | 'dark';
export type ColorScheme = 'light' | 'dark';

/** Même clé que `public/theme.js`, qui pose le thème avant le premier rendu. */
const KEY = 'robiade.theme';
const NEXT: Record<ThemeChoice, ThemeChoice> = { system: 'light', light: 'dark', dark: 'system' };

function stored(): ThemeChoice {
  try {
    const choice = localStorage.getItem(KEY);
    return choice === 'light' || choice === 'dark' ? choice : 'system';
  } catch {
    return 'system';
  }
}

const systemDark = window.matchMedia('(prefers-color-scheme: dark)');
export const theme = $state({ choice: stored(), systemDark: systemDark.matches });
systemDark.addEventListener('change', (e) => (theme.systemDark = e.matches));

/**
 * Thème effectif, pour ce que `color-scheme` n'atteint pas : les iframes `srcdoc`.
 * WebKit ne leur transmet pas celui de la page ; laissées à `light dark`, elles suivraient le système.
 */
export function colorScheme(): ColorScheme {
  if (theme.choice !== 'system') return theme.choice;
  return theme.systemDark ? 'dark' : 'light';
}

/** Système → Clair → Sombre → Système. L'attribut `data-theme` fixe `color-scheme` (app.css). */
export function cycleTheme(): void {
  const choice = NEXT[theme.choice];
  theme.choice = choice;
  if (choice === 'system') delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = choice;
  try {
    if (choice === 'system') localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, choice);
  } catch {
    // Stockage inaccessible : le choix vaut pour la session.
  }
}
