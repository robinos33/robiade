# UX

**En une phrase** : les principes d'interface, la structure de l'écran et les budgets de
performance, communs à toutes les vues.

## Comportement

### Structure de l'écran
```
┌─────────┬──────────────────────────────────────────┬─────────────────┐
│ Inbox   │  Vue courante (Agents | MR | Issues |    │  Détail         │
│ ─────── │  Environnements | DB | Stats)            │  (objet         │
│ Global  │                                          │   sélectionné,  │
│ ─────── │  liste virtualisée, filtrable            │   terminal      │
│ Projets │                                          │   d'agent,      │
│  ade    │                                          │   diff, logs)   │
│  shop   │                                          │                 │
│  forum  │                                          │                 │
└─────────┴──────────────────────────────────────────┴─────────────────┘
```
- **Inbox** = la [vue Status](../inbox.md). C'est l'écran d'accueil.
- Sélectionner un projet filtre toutes les vues ; « Global » enlève le filtre.
- Le panneau de détail d'un agent héberge son terminal ; on peut le détacher dans une fenêtre.
- Dans le terminal, ⌘+clic ouvre un lien, comme dans iTerm ; ⌘ enfoncé, le lien survolé est
  souligné. Sans ⌘, un clic reste un clic dans le terminal. Claude Code y affiche ses URL en
  clair (`FORCE_HYPERLINK=0`) : xterm 6 ne laisse pas ⌘ décider du soulignement d'un lien OSC 8.
- Dans le terminal d'un agent, le clic molette colle la dernière sélection dans le prompt, sans
  l'envoyer, comme la sélection primaire de X11. Cette sélection vient d'un terminal, celui-ci ou
  un autre, ou d'une zone sélectionnable de l'interface : titre d'issue ou de MR, diff,
  historique, chemin de fichier. Le reste de l'interface ne se sélectionne pas, grille DB
  comprise. Une fin de ligne finale est retirée, et le presse-papiers n'est ni lu ni modifié.
  Avant toute sélection, le clic molette ne fait rien. Une fenêtre détachée ne voit que les
  sélections faites dans cette fenêtre.

## Règles

### Principes
- **Clavier d'abord.** Palette `⌘K` pour tout (aller à un projet, une MR, une session ; lancer
  une action). Chaque action fréquente a un raccourci ; la souris reste optionnelle.
- **Densité maîtrisée.** Une ligne = un objet, avec l'état lisible sans survol. Pas de cartes
  aérées façon dashboard marketing.
- **L'état d'abord, la décoration ensuite.** Le seul code couleur fort est réservé à ce qui
  demande une action ([Inbox](../inbox.md)). Le reste est neutre.
- **Rien ne saute.** Les mises à jour en direct ne déplacent pas la ligne sous le curseur ; le
  tri ne change pas pendant qu'on lit.
- Thème Système (par défaut, suit macOS à chaud), Clair ou Sombre, choisi par un bouton en bas
  de la barre latérale et mémorisé sur le poste ; le terminal garde Catppuccin Macchiato dans les
  deux. Respect de « Réduire les animations ».
- **Écran de démarrage** : le logo en grand, le point qui en fait le tour, sans texte. Il couvre
  l'interface pendant qu'elle se remplit dessous (environnement, agents, tâches, historique), au
  moins un tour du point (900 ms, pour ne pas flasher) et **jamais plus de 5 s** : un démarrage
  lent ou bloqué rend la main quand même. Un échec le retire aussi, « Démarrage incomplet » dit
  pourquoi.

### Budgets de performance (mesurés, pas supposés)
| Mesure | Cible |
|---|---|
| Démarrage à froid jusqu'à l'interface prête (fin de `init()`, minimum de l'écran de démarrage exclu) | < 500 ms |
| RAM au repos, 10 sessions suivies | < 200 Mo (process Rust + WebView) |
| Latence d'un événement de hook jusqu'à l'écran | < 100 ms |
| Scroll d'une liste de 5 000 éléments | 60 i/s sans saccade |
| Recherche plein texte dans l'historique | < 150 ms |

### Terminal
- **Rien ne part sans Entrée.** Un collage n'ajoute jamais de retour chariot et passe en bracketed
  paste. Si le programme ne l'a pas activé, un texte sur plusieurs lignes n'est pas collé : chaque
  ligne serait envoyée.

## Décisions

- Décidé le 2026-11-03 : le clic molette colle la dernière sélection même quand elle n'est plus
  en surbrillance, ou après un changement de vue (issue #12). Le terminal n'est affiché que dans
  la vue Agents, et quitter une vue efface sa sélection : sans cela, on ne pourrait rien reprendre
  d'une autre vue. Risque résiduel : un texte oublié arrive dans le prompt, mais il ne part pas
  sans Entrée.

## Écarté

- Ne coller qu'une sélection encore visible, comme le demandait l'issue #12 : un titre d'issue ou
  un diff d'une autre vue n'aurait jamais pu être repris.

## Tickets

- #10 — Interface : bouton de thème, système, clair, sombre
- #11 — Terminal : ⌘+clic sur un lien
- #12 — Terminal : clic molette colle la sélection dans le prompt
- #21 — Interface : écran de démarrage
