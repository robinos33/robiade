# Worktrees

**En une phrase** : les worktrees de chaque projet, leur état, et leur création ou suppression.

## Comportement

- **Worktrees** : liste par projet, branche, propreté (fichiers modifiés), agent actif dessus,
  création et suppression (refusée si un agent est actif ou le worktree verrouillé ; un arbre
  modifié liste ses fichiers et ne part qu'une fois leur perte cochée).
- **Base d'un nouveau worktree** : les branches de tronc du dépôt ([réglages](../socle/reglages.md))
  qui existent sur `origin`, dans leur ordre, la première proposée par défaut. Sans réglage,
  `origin/HEAD`. Si aucune n'existe, la création est refusée avec la marche à suivre
  (`git remote set-head origin --auto`, ou régler les troncs). Une base n'est acceptée que si elle
  fait toujours partie de cette liste au moment de créer.
- **Colonne Branche** : la branche est une colonne alignée face au nom, sur toutes les lignes.
  Elle est coupée par le début quand elle déborde (la fin distingue les branches), avec le nom
  complet au survol. Elle s'affiche même identique au nom du worktree ; une tête détachée est
  en italique atténué, un worktree pas encore relu par git montre un tiret.
- **Fermer les propres (N)** : ferme d'un coup les worktrees sans modification, sans agent actif
  et non verrouillés, sur le périmètre affiché (le projet courant, ou tous en vue globale). La
  confirmation les liste avec leur branche, sans case à cocher : rien ne se perd, les branches
  restent. Chacun passe par la même suppression que le dialogue unitaire, qui revérifie tout :
  un arbre modifié entre l'ouverture et le clic est refusé. Si le worktree sélectionné en fait
  partie, la sélection revient au projet.

## Règles

- La suppression, unitaire ou en lot, ne touche jamais à une branche.
- Risque résiduel : « agent actif » ne compte que les agents lancés par Robiade. Un Claude Code
  ouvert dans un terminal sur un worktree propre ne le protège pas, du dialogue unitaire comme
  du lot.

## Décisions

- Décidé le 2026-11-04 : un lot entièrement fermé s'annonce par une notification ; au moindre
  refus, le dialogue reste ouvert avec chaque refus et sa raison, qu'une notification de
  quelques secondes ne laisse pas lire (issue #29).

## Écarté

- Fermer en lot des worktrees modifiés : ça reste le dialogue unitaire, qui liste les fichiers
  perdus.
- Supprimer les branches, même mergées, avec leur worktree.
- Détecter automatiquement les worktrees « finis » (branche mergée, issue fermée).

## Tickets

- #26 — Socle — indépendance vis-à-vis de l'entreprise : instance GitLab, Vault et branches de tronc dans une page Réglages
- #29 — Environnements — branche en colonne face au worktree, et fermer tous les worktrees propres d'un coup
