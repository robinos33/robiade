# Issues

**En une phrase** : les issues de la forge sont les tâches d'équipe ; l'ADE les montre, les
modifie sur la forge et peut lancer un agent dessus.

## Comportement

**Issues = tâches** : seulement celles qui me sont assignées, par projet ou tous projets confondus.
Je suis l'utilisateur du jeton de la forge : rien à régler, et c'est la forge qui filtre, pas
l'ADE après lecture (la liste tient en une page). Création, changement d'état, assignation,
labels : tout part vers GitLab, l'ADE ne garde rien. Sur GitHub, une issue n'a ni échéance ni décompte de MR liées, et une pull request
n'est jamais lue comme une issue. Une issue peut lancer une session (« démarrer un agent sur cette issue » : worktree +
branche + prompt initial pré-rempli avec l'issue, que je relis avant envoi).
Une issue se lit dans le même panneau qu'une MR ([merge requests](merge-requests.md)).

## Décisions

Décidé le 2026-11-10 : la vue d'un projet ne montre plus toutes ses issues ouvertes, seulement
celles qui me sont assignées, comme la vue globale. Risque résiduel : une issue créée depuis
l'ADE sans m'assigner disparaît de la liste une fois créée.

## Tickets

- #28 — Forge — fournisseur GitHub (pull requests, issues, checks)
