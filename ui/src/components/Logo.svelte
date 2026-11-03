<script lang="ts">
  /**
   * Le carré « R » et son point d'accent, qui en fait le tour : une fois au survol (barre latérale),
   * ou en boucle (écran de démarrage). Toutes les cotes suivent `size`.
   */
  let { size = 22, orbit = 'hover' }: { size?: number; orbit?: 'hover' | 'loop' } = $props();
</script>

<span class="logo" class:loop={orbit === 'loop'} style:--s="{size}px" aria-hidden="true">R</span>

<style>
  /*
   * Cotes dessinées pour 22 px, mises à l'échelle : point de 7, qui déborde de 2 du coin, rayon 6, police 13.
   * `--travel` sépare deux coins sur la course du point : 19 px à 22 px.
   */
  .logo {
    --dot: calc(var(--s) * 7 / 22);
    --out: calc(var(--s) * 2 / 22);
    --travel: calc(var(--s) - var(--dot) + 2 * var(--out));
    display: grid; place-items: center; width: var(--s); height: var(--s); border-radius: calc(var(--s) * 6 / 22);
    background: var(--ink); color: var(--bg); font-weight: 800; font-size: calc(var(--s) * 13 / 22); position: relative;
  }
  /* Le halo détache le point du fond sur lequel le logo est posé : `--logo-halo`, la barre latérale par défaut. */
  .logo::after {
    content: ''; position: absolute; top: calc(-1 * var(--out)); right: calc(-1 * var(--out));
    width: var(--dot); height: var(--dot); border-radius: 50%; background: var(--accent);
    box-shadow: 0 0 0 var(--out) var(--logo-halo, var(--bg-raised));
  }
  /* Le point fait le tour du carré dans le sens horaire : une fois au survol, ou sans fin. */
  .logo:not(.loop):hover::after { animation: orbit 900ms linear; }
  .loop::after { animation: orbit 900ms linear infinite; }
  @keyframes orbit {
    25% { transform: translate(0, var(--travel)); }
    50% { transform: translate(calc(-1 * var(--travel)), var(--travel)); }
    75% { transform: translate(calc(-1 * var(--travel)), 0); }
  }
</style>
