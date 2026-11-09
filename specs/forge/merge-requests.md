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

**Fournisseurs** : GitLab ou GitHub, réglés dans les [réglages](../socle/reglages.md). Sur GitHub,
une pull request est une MR, numérotée `#N`, et son projet est `owner/repo`. Le pipeline est la
synthèse des checks et des statuts du dernier commit ; relancer un job relance les jobs en échec
d'un workflow, celui d'un check en échec. Les approbations suivent la décision de review de
GitHub, et le nombre requis est le plus exigeant de la protection de branche et des rulesets
lisibles. Les messages des bots sont traités comme des notes système. Dans le panneau, un fil de
review n'a pas d'état résolu (l'API REST ne le donne pas), mais la liste signale les discussions non
résolues.

Lien Agents ↔ Forge : une session porteuse d'un `pr-link` ou dont la branche correspond à une MR
est affichée **sur** la MR, et inversement. La MR d'un agent ouvert se lit, elle, par la branche extraite
dans son dossier : la plus récente qui en part, **quel que soit son état** (mergée, fermée), avec
son pipeline et ses approbations, sur les deux forges. Une branche de tronc n'a pas de MR d'agent.
Relue au plus toutes les 30 s, et après chaque action lancée depuis Robiade. Elle nourrit la
[frise du ticket](../agents/status.md). Risque résiduel (GitHub) : la PR d'un fork, dont la
branche vit chez un autre propriétaire, n'est pas trouvée.

## Règles

Règles de domaine portées par le code, pas par l'utilisateur :
- un wagon intermédiaire (cible hors des branches de tronc de son projet) a `squash = false`. Les
  troncs sont ceux du dépôt local qui porte le projet, réglés dans les [réglages](../socle/reglages.md) ;
  sans réglage, la branche par défaut du projet lue sur la forge. Si elle est illisible, la MR
  n'est pas marquée comme wagon dans la liste, et le merge est refusé plutôt que deviné ;
- un merge ne part que sur le commit de tête vu à la confirmation (`sha`), relu juste avant ;
- draft/ready : sur GitLab par le titre (`Draft: `), relu au moment d'agir ; sur GitHub par
  l'état brouillon de la PR. La confirmation dit « marquer prête » ou « repasser en brouillon » ;
- un merge que le dépôt interdit (squash désactivé sur GitHub pour un tronc, merge commit
  désactivé pour un wagon) est refusé dès la confirmation, rien ne part. GitLab tranche lui-même à
  l'appel ;
- rebaser une PR GitHub la rebase sur sa cible ; elle ne merge jamais la cible dans la branche ;
- jamais de merge du parent dans un wagon ;
- ces règles ne s'appliquent qu'à mes MR.
- Sur GitHub, des approbations exigées mais dont le nombre est illisible avec le jeton sont
  « inconnues », jamais « approuvée ».
- **Risque résiduel** (GitHub) : une review en attente disparaît de la liste dès que j'en rends
  une, approbation ou simple commentaire. Mes PR et les reviews en attente sont lues par la
  recherche, 100 au plus.

## Décisions

Décidé le 2026-11-04 : **sans réglage, le tronc d'une MR est la `default_branch` du projet sur la
forge**, même pour un dépôt cloné (issue #26, qui proposait `origin/HEAD` en local). Une MR vit sur
le remote ; `origin/HEAD` peut manquer (jamais posé après un `git remote add`) ou dater. Lue une
fois par lancement et par projet.

## Tickets

- #18 — Forge : lire une issue ou une MR dans Robiade, sans ouvrir GitLab
- #26 — Socle — indépendance vis-à-vis de l'entreprise : instance GitLab, Vault et branches de tronc dans une page Réglages
- #28 — Forge — fournisseur GitHub (pull requests, issues, checks)
- #51 — Agents — frise du cycle de vie du ticket, fixée sous le terminal
