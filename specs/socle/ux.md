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
- Dans la barre latérale, un clic sur le titre `Projets` ou `Worktrees` replie la section ; le
  repli est mémorisé sur le poste. Repliée, elle montre son nombre d'éléments et la pastille la
  plus forte de ses lignes (un agent qui attend reste visible), garde la ligne de la sélection
  et rend sa hauteur à l'autre section. La navigation ne se replie pas : le clavier la couvre.
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
- ⌘+clic ouvre aussi un chemin que Claude Code cite sans `file://` (`src/foo.rs`,
  `src/foo.rs:42:7`), relatif au dossier de l'agent, absolu ou sous `~/`. Le visualiseur s'ouvre
  sur la ligne citée, surlignée ; un Markdown ou un HTML s'ouvre alors en source. Un mot n'est un
  lien que s'il désigne un fichier que ce ⌘+clic lirait : la question part au survol d'une ligne,
  jamais pour les lignes simplement affichées.
- ⌘F ouvre une barre de recherche en haut à droite du terminal : occurrences surlignées, compteur
  « 3 / 12 », Entrée et ⇧Entrée pour suivante et précédente, Échap pour fermer et rendre le
  clavier au terminal.
- Des fichiers déposés depuis le Finder sur le terminal d'un agent y insèrent leurs chemins
  absolus, échappés par des antislashs comme dans iTerm, séparés et suivis d'une espace, sans
  Entrée : c'est ainsi qu'on donne une capture d'écran à Claude Code. Le dépôt vise le terminal
  sous le curseur, qu'un liseré marque pendant le glisser.
- ⌘+, ⌘− et ⌘0 règlent la police de tous les terminaux d'agent, de 9 à 24 px, 13 par défaut ; le
  réglage est mémorisé sur le poste, et le reste de l'interface ne zoome pas.
- **Panneaux latéraux redimensionnables** : barre latérale, liste et panneau Session/Diff des
  agents, arbre de Code, liste de l'historique, logs d'un conteneur, arbre et détail d'une ligne
  de l'explorateur DB. Une poignée sur la bordure, surlignée au survol, se glisse ou se règle
  aux flèches ← →. Un double-clic rend la largeur par défaut. La largeur est retenue sur le
  poste, une par panneau. Les modales ne sont pas concernées.

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
- **Un panneau reste entre ses bornes** : un minimum pour que son contenu reste utilisable, un
  maximum pour que la zone principale garde le sien (le terminal d'agent garde environ
  60 colonnes). Une largeur retenue qui ne tient plus, sur un écran plus petit, s'affiche bornée
  et revient quand la place revient.

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
  ligne serait envoyée. Les caractères de contrôle sont retirés avant le collage, sauf tabulation
  et fins de ligne : un titre d'issue venu de GitLab pourrait sinon fermer le bracketed paste et
  envoyer la suite.
- **Le terminal se recale une fois, au relâchement d'une poignée.** Chaque recalage envoie un
  redimensionnement au PTY et fait redessiner tout l'écran à Claude Code.
- Les raccourcis de Robiade (⌘F, ⌘+, ⌘−, ⌘0) ne partent jamais dans le PTY.
- Un dépôt de fichiers dont un nom contient un caractère de contrôle, retour à la ligne compris,
  est refusé en entier, avec un message : l'antislash ne le neutraliserait pas.

## Décisions

- Décidé le 2026-11-03 : le clic molette colle la dernière sélection même quand elle n'est plus
  en surbrillance, ou après un changement de vue (issue #12). Le terminal n'est affiché que dans
  la vue Agents, et quitter une vue efface sa sélection : sans cela, on ne pourrait rien reprendre
  d'une autre vue. Risque résiduel : un texte oublié arrive dans le prompt, mais il ne part pas
  sans Entrée.
- Décidé le 2026-11-03 : sans réglage, un panneau garde la largeur de la feuille de style, y
  compris quand elle est proportionnelle (26 %, 38 %). Seul un glisser ou une flèche fixe une
  largeur en pixels (issue #23).
- Décidé le 2026-11-03 : les bornes ne s'appliquent qu'à l'affichage, la largeur retenue n'est
  réécrite que par l'utilisateur (issue #23).
- Décidé le 2026-11-03 : une seule largeur pour les onglets Session et Diff, pour que changer
  d'onglet ne redimensionne pas le terminal (issue #23).
- **[À trancher]** Dans l'onglet Agents, la liste se borne en supposant le panneau Session/Diff
  à son minimum, et celui-ci prend la place qu'elle laisse : élargir la liste rétrécit le
  panneau de droite. C'est le choix fait pendant le ticket, parce que des bornes qui dépendent
  l'une de l'autre oscillent. Je ne l'ai pas encore validé (issue #23).

- Décidé le 2026-11-03 : un chemin cité est vérifié au survol de sa ligne, ⌘ enfoncé ou non,
  alors que l'issue #13 demandait la vérification au survol avec ⌘. xterm garde les liens d'une
  ligne tant que la souris y reste : un ⌘ enfoncé sans bouger ne relancerait pas la vérification.
  Coût : une question à Rust par ligne survolée.
- Décidé le 2026-11-03 : les chemins déposés sont échappés par des antislashs, comme iTerm, plutôt
  qu'entourés de `'…'` (issue #13). Claude Code reconnaît déjà ainsi une image déposée.
- Décidé le 2026-11-03 : la recherche du terminal surligne ses occurrences par l'API proposée
  d'xterm 6 (`allowProposedApi`), sans laquelle ni surbrillance ni compteur (issue #13). Risque
  résiduel : cette API peut changer à une mise à jour d'xterm.
- Décidé le 2026-11-04 : `Projets` repliée garde la ligne du projet sélectionné, ou « Global »,
  même quand c'est un worktree qui est sélectionné (issue #27). Le projet reste le contexte : sans
  elle, la section ne dirait plus où l'on est dès qu'on entre dans un worktree.

## Écarté

- Ne coller qu'une sélection encore visible, comme le demandait l'issue #12 : un titre d'issue ou
  un diff d'une autre vue n'aurait jamais pu être repris.
- Recalage du terminal à chaque image (`requestAnimationFrame`) pendant le glisser : encore un
  redimensionnement du PTY par colonne franchie, soit des dizaines par glisser (issue #23).

## Tickets

- #10 — Interface : bouton de thème, système, clair, sombre
- #11 — Terminal : ⌘+clic sur un lien
- #12 — Terminal : clic molette colle la sélection dans le prompt
- #13 — Terminal : chemins cliquables, ⌘F, glisser-déposer, taille de police
- #21 — Interface : écran de démarrage
- #23 — Interface — panneaux latéraux redimensionnables
- #27 — Barre latérale — replier les sections Projets et Worktrees
