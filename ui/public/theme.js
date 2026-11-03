// Pose le thème forcé avant le premier rendu : script classique, donc bloquant, chargé depuis <head>.
// Hors bundle parce que la CSP interdit le script inline. Même clé que src/lib/theme.svelte.ts.
try {
  const choice = localStorage.getItem('robiade.theme');
  if (choice === 'light' || choice === 'dark') document.documentElement.dataset.theme = choice;
} catch {
  // Stockage inaccessible : on suit le système.
}
