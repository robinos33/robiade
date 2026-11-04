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

## Tickets

- #26 — Socle — indépendance vis-à-vis de l'entreprise : instance GitLab, Vault et branches de tronc dans une page Réglages
