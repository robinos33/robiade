# Issues

**En une phrase** : les issues GitLab sont les tâches d'équipe ; l'ADE les montre, les modifie
dans GitLab et peut lancer un agent dessus.

## Comportement

**Issues = tâches** : vue par projet, et vue globale (issues qui me sont assignées, tous projets
confondus). Création, changement d'état, assignation, labels : tout part vers GitLab, l'ADE ne
garde rien. Une issue peut lancer une session (« démarrer un agent sur cette issue » : worktree +
branche + prompt initial pré-rempli avec l'issue, que je relis avant envoi).
Une issue se lit dans le même panneau qu'une MR ([merge requests](merge-requests.md)).
