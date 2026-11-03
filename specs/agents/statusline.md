# Statusline

**En une phrase** : le contexte consommé et les quotas d'un agent viennent de la statusline que
Robiade déclare pour lui, et s'affichent dans une barre native.

## Décisions

Décidé le 2026-10-27 (après coup) : **contexte consommé et quotas lus par la statusline.** Robiade
déclare sa propre `statusLine` dans le `--settings` de ses agents ; `robiade statusline` relaie le
JSON sur le socket des hooks et n'écrit rien dans le terminal. L'interface dessine une barre native
(modèle, contexte, quotas 5 h et semaine, seuils 50 % / 80 %). La statusline de `~/.claude` reste
celle d'iTerm. Les champs lus viennent de la statusline existante, pas d'un contrat documenté :
même règle de tolérance que pour les transcripts.
