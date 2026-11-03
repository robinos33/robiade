# Merge requests

**En une phrase** : suivre mes MR et celles qui attendent ma review, et agir dessus sous
confirmation, avec les règles des wagons portées par le code.

## Comportement

**Lecture** : mes MR (auteur ou relecteur), état pipeline, discussions non résolues, conflits,
approbations ; MR des autres en attente de ma review. Une issue ou une MR se lit en entier dans un
panneau, à côté de la liste : description et fils d'échanges (sans les notes système), avec leur
état résolu ou non. Lus à l'ouverture, jamais stockés ; ⌘+clic garde le navigateur.

**Actions** (toutes confirmées dans l'UI, voir la [sécurité](../socle/securite.md)) : commenter, approuver, rebaser, relancer un job,
passer draft/ready, merger.

Lien Agents ↔ Forge : une session porteuse d'un `pr-link` ou dont la branche correspond à une MR
est affichée **sur** la MR, et inversement.

## Règles

Règles de domaine portées par le code, pas par l'utilisateur :
- un wagon intermédiaire (cible ≠ `staging`/`master`) a `squash = false` ;
- un merge ne part que sur le commit de tête vu à la confirmation (`sha`), relu juste avant ;
- draft/ready passe par le titre (`Draft: `), seule bascule que l'API expose sans ambiguïté ;
- jamais de merge du parent dans un wagon ;
- ces règles ne s'appliquent qu'à mes MR.

## Tickets

- #18 — Forge : lire une issue ou une MR dans Robiade, sans ouvrir GitLab
