/**
 * Vrai pendant le glisser d'une poignée de panneau. Les terminaux suspendent leur `fit` et le
 * refont une fois au relâchement : chaque `fit` d'un agent envoie un `resize_agent` au PTY, et
 * Claude Code redessine tout l'écran à chacun.
 */
export const panelDrag = $state({ active: false });
