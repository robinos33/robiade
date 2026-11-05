# Activité de la forge

**En une phrase** : mes contributions sur la forge, MR ouvertes et mergées, commits et reviews,
par jour, semaine, mois ou année.

## Comportement

**Activité** (mes contributions, par jour / semaine / mois / année) : MR ouvertes, MR
mergées, commits, reviews faites. Sources : mes événements GitLab (`/events`) et mes MR mergées
(`/merge_requests?scope=created_by_me&state=merged`), tous projets confondus — un événement ne porte que l'identifiant de son projet, la vue ne suit donc pas le
filtre de projet. Un événement a pour auteur celui qui agit, d'où ce que compte chaque série
(décidé le 2026-11-02) :
- MR ouvertes : `opened` sur une MR ;
- MR mergées : **mes** MR, mergées par moi ou par n'importe qui, à la date de leur merge. Pas
  l'événement `accepted` : il appartient à celui qui merge, il manquerait les merges des collègues ;
  une MR d'un autre que j'ai mergée ne compte pas ;
- commits : `commit_count` de chaque push ; un push forcé après rebase les recompte, c'est un
  volume poussé ;
- reviews : `approved` sur une MR. Un commentaire ne compte pas : l'événement ne dit pas à qui est
  la MR, et répondre sur la sienne n'est pas une review.

Fenêtres : 30 jours, 26 semaines (du lundi), 12 mois, 3 ans (GitLab ne garde pas les événements
au-delà). Lecture par tranches de 30 jours, en parallèle, gardées en mémoire : une tranche lue plus
d'un jour après sa fin ne change plus, celle en cours est relue après 5 minutes. Rien sur disque :
le cache se reconstruit au premier affichage après un lancement. Coût constaté : ~15 pages de
100 événements par mois chargé, donc quelques secondes pour 26 semaines et de l'ordre de la minute
pour 3 ans, une fois par lancement.

**Sur GitHub**, mêmes séries : PR ouvertes, reviews `approved` et pushes viennent de mes
événements, PR mergées de la recherche (mes PR, quel que soit celui qui merge). GitHub ne sert que
mes 300 derniers événements, sur 90 jours : au-delà, commits, PR ouvertes et reviews sont vides, et
la vue le dit. Un push sans décompte de commits en compte un, le minimum certain.

## Tickets

- #28 — Forge — fournisseur GitHub (pull requests, issues, checks)
