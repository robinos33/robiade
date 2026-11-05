import { app } from './state.svelte';

/**
 * Terminal d'agent en pleine fenêtre : barre latérale, en-tête, liste et panneau Session/Diff
 * s'effacent. Mode passager, non mémorisé ; il survit au changement d'agent et de vue, mais ne
 * s'applique que dans la vue Agents, et seulement s'il y a un terminal à montrer.
 */
class TerminalFull {
  on = $state(false);
  shown = $derived(this.on && app.view === 'agents' && app.visibleAgents.length > 0);

  /**
   * Bascule ce qui se voit : depuis la palette, hors de la vue Agents, le mode s'allume même s'il
   * était resté allumé en arrière-plan. Instantané : le terminal ne se recale qu'une fois, au
   * `ResizeObserver`.
   */
  toggle = () => {
    this.on = !this.shown;
  };
}

export const terminalFull = new TerminalFull();
