# Worktrees

**En une phrase** : les worktrees de chaque projet, leur état, et leur création ou suppression.

## Comportement

- **Worktrees** : liste par projet, branche, propreté (fichiers modifiés), agent actif dessus,
  création et suppression (refusée si un agent est actif ou le worktree verrouillé ; un arbre
  modifié liste ses fichiers et ne part qu'une fois leur perte cochée).
