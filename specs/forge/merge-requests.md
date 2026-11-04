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
- un wagon intermédiaire (cible hors des branches de tronc de son projet) a `squash = false`. Les
  troncs sont ceux du dépôt local qui porte le projet, réglés dans les [réglages](../socle/reglages.md) ;
  sans réglage, la branche par défaut du projet lue sur la forge. Si elle est illisible, la MR
  n'est pas marquée comme wagon dans la liste, et le merge est refusé plutôt que deviné ;
- un merge ne part que sur le commit de tête vu à la confirmation (`sha`), relu juste avant ;
- draft/ready passe par le titre (`Draft: `), seule bascule que l'API expose sans ambiguïté ;
- jamais de merge du parent dans un wagon ;
- ces règles ne s'appliquent qu'à mes MR.

## Décisions

Décidé le 2026-11-04 : **sans réglage, le tronc d'une MR est la `default_branch` du projet sur la
forge**, même pour un dépôt cloné (issue #26, qui proposait `origin/HEAD` en local). Une MR vit sur
le remote ; `origin/HEAD` peut manquer (jamais posé après un `git remote add`) ou dater. Lue une
fois par lancement et par projet.

## Tickets

- #18 — Forge : lire une issue ou une MR dans Robiade, sans ouvrir GitLab
- #26 — Socle — indépendance vis-à-vis de l'entreprise : instance GitLab, Vault et branches de tronc dans une page Réglages
