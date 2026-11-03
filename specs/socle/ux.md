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
- **Inbox** = la vue Status (§4.5). C'est l'écran d'accueil.
- Sélectionner un projet filtre toutes les vues ; « Global » enlève le filtre.
- Le panneau de détail d'un agent héberge son terminal ; on peut le détacher dans une fenêtre.
- Dans le terminal, ⌘+clic ouvre un lien, comme dans iTerm ; ⌘ enfoncé, le lien survolé est
  souligné. Sans ⌘, un clic reste un clic dans le terminal. Claude Code y affiche ses URL en
  clair (`FORCE_HYPERLINK=0`) : xterm 6 ne laisse pas ⌘ décider du soulignement d'un lien OSC 8.

## Règles

### Principes
- **Clavier d'abord.** Palette `⌘K` pour tout (aller à un projet, une MR, une session ; lancer
  une action). Chaque action fréquente a un raccourci ; la souris reste optionnelle.
- **Densité maîtrisée.** Une ligne = un objet, avec l'état lisible sans survol. Pas de cartes
  aérées façon dashboard marketing.
- **L'état d'abord, la décoration ensuite.** Le seul code couleur fort est réservé à ce qui
  demande une action (§4.5). Le reste est neutre.
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

## Tickets

- #10 — Interface : bouton de thème, système, clair, sombre
- #11 — Terminal : ⌘+clic sur un lien
- #21 — Interface : écran de démarrage
