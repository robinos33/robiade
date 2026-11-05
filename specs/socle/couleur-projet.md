# Couleur de projet

**En une phrase** : chaque projet peut recevoir une couleur, réglée dans Réglages, qui le
distingue d'un coup d'œil partout où il apparaît ; c'est une identité, jamais un état.

## Comportement

- La couleur se choisit dans [Réglages](reglages.md), section Projets, à côté des branches de
  tronc : « Aucune » ou l'une des sept teintes d'une palette fixe (olive, vert, sarcelle, ciel,
  bleu, violet, rose), au clic ou aux flèches. Elle s'applique partout dès le choix, sans relance.
- « Aucune » est le défaut : un projet non réglé reste neutre.
- Rattachée au dépôt principal, comme les troncs : ses worktrees prennent sa couleur. Un objet de
  forge (MR, issue) prend celle du dépôt local qui porte son projet ; sans dépôt local, aucune.
- **Là où est le nom d'un projet, est sa couleur** : un trait vertical devant le nom (composant
  `ProjectTag`). Il sert dans les listes (MR, issues, tâches, environnements, historique, Inbox,
  Stats), la palette ⌘K, le panneau de lecture et les dialogues de confirmation. Ailleurs, la
  marque prend la forme du lieu :
  - **onglets d'agents**, En cours et Récents, et **barre du terminal** (donc aussi en pleine
    fenêtre) : un liseré de 3 px à gauche, qui se lit sans lire le texte ;
  - **barre latérale** et « Nouvel agent dans… » : l'icône du projet ou du worktree ; le nom du
    projet à côté d'un worktree, en vue globale, reste du texte, pour ne pas marquer deux fois la
    même ligne ;
  - **projet sélectionné** : un filet de 2 px à sa couleur sous l'en-tête de chaque vue rappelle
    que le filtre est actif.
- Sans couleur, aucune marque : ni liseré, ni trait, ni filet.

## Règles

- La couleur de projet n'est **jamais ronde** (le rond porte l'état), **jamais un fond plein** (le
  fond plein porte la sélection), et **jamais seule** : le nom l'accompagne ou est à un survol près.
- Elle n'entre jamais dans un compteur ni dans une pastille d'attente : un agent qui attend reste
  signalé par l'accent, quel que soit son projet.
- La palette exclut l'orange de l'action, le rouge du danger, l'ambre de l'attente et le gris
  neutre, qui se lirait « aucune ». Chaque teinte est définie pour les deux thèmes, avec un
  contraste d'au moins 3,9:1 sur le fond.
- Pas de couleur libre : le domaine refuse toute teinte hors de la palette, et une teinte lue en
  base qui n'y est plus vaut « aucune ».

## Décisions

- **[À trancher]** Défaut « Aucune » plutôt qu'une couleur attribuée par hachage du nom, et
  palette fixe plutôt que sélecteur libre : ce sont les recommandations de l'issue #37, suivies
  pendant le ticket. Je ne les ai pas encore validées.
- **[À trancher]** Sept teintes au lieu des huit environ de l'issue #37 : mesurée en ΔE, une
  huitième tombait près du rouge du danger, de l'ambre de l'attente ou du gris neutre. Choix fait
  pendant le ticket, pas encore validé.
- **[À trancher]** Les onglets En cours portaient un liseré orange fixe, le même pour tous ; il
  laisse la place au liseré du projet, transparent sans couleur, pour ne pas en avoir deux côte à
  côte et laisser l'accent à ce qui attend (issue #37). Choix fait pendant le ticket, pas encore
  validé.

## Écarté

- Thème complet par projet (fond, accent de l'interface) : on perdrait le code couleur de l'état.
- Couleur par worktree ou par agent : les worktrees héritent de leur projet.
- Couleur dans le contenu du terminal (prompt, sortie de Claude Code).
- Synchroniser la couleur avec un réglage extérieur (avatar de forge, couleur d'IDE).

## Tickets

- #37 — Projets — une couleur par projet, réglable, reprise partout où le projet apparaît
