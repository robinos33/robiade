# Tâches perso

**En une phrase** : mes tâches, par projet ou globales, dont l'ADE est la source de vérité ;
elles peuvent référencer une issue, se lier à une session et lancer un agent.

## Comportement

C'est le seul contexte où l'ADE porte de l'état, donc le seul où un agrégat a un sens.
- **Échéance** : un jour du calendrier (`AAAA-MM-JJ`), sans heure ni fuseau, saisi au champ date.
  Une tâche pas encore faite dont l'échéance est aujourd'hui ou passée (jour local) est **tombée** :
  date en rouge sur la carte, pastille et compte sur l'entrée « Tâches », à la place du compte
  des tâches en cours. La pastille est **toujours** là, quel que soit le projet affiché — une tâche
  globale (« visite médicale ») ne doit pas disparaître parce qu'on travaille dans un projet ;
  le **nombre** compte les tâches du projet affiché, et une échéance tombée seulement ailleurs
  laisse une pastille sans nombre.
- Le lien vers une session ouvre l'agent s'il tourne encore, sinon la session dans l'historique.
- **Démarrer un agent depuis une tâche** pas encore faite : même chemin que depuis une issue
  (branche, worktree, prompt relu avant envoi). Une tâche liée à une issue part du dépôt de
  l'issue, de sa branche `ticket-<iid>-…` et de son prompt ; sinon, du dépôt de son projet et
  d'une branche `tache-<titre>`. Mes notes s'ajoutent au prompt. Une fois l'agent lancé, la tâche
  est liée à sa session et passe en cours. Une tâche globale, sans dépôt, n'en propose pas.
- **Un agent ferme sa tâche, quand je le lui demande** : `robiade task done`, sans argument, passe
  en `Done` la tâche liée à sa session, et elle seule. Refusé (sortie ≠ 0) si aucune tâche n'y
  est liée, si elle est déjà finie ou si plusieurs le sont. La vue Tâches se relit à son ouverture
  et au retour dans l'app.
- Vue unifiée : la liste des tâches d'un projet montre les deux types côte à côte, typés
  visuellement ; seules les tâches perso sont éditables en place.

## Règles

- **Agrégat `Task`** : identifiant, titre (non vide, ≤ 200 caractères), notes, projet optionnel
  (absent = tâche globale), statut `Todo → Doing → Done` (retour arrière autorisé, `Done` horodaté),
  échéance optionnelle, liens optionnels vers une session ou une issue GitLab.
- Une tâche perso peut **référencer** une issue d'équipe, jamais la dupliquer : pas de
  synchronisation, pas de champ recopié. La référence se saisit comme GitLab l'écrit (`#12` dans
  le projet de la tâche, `groupe/projet#12`, ou l'URL) et ne garde que le projet et le numéro ;
  le titre affiché au survol vient des issues déjà lues, sinon rien.

## Décisions

Décidé le 2026-11-03 : **un agent ferme sa tâche par `robiade task done`** (issue #17), plutôt
qu'un `UPDATE` à la main dans `robiade.db` qui contournait l'agrégat. La tâche se cherche par
**session**, pas par agent : un agent repris reçoit un identifiant neuf, mais garde la session
d'origine à laquelle sa tâche est liée. L'app pose au lancement `ROBIADE_SESSION_ID`, `ROBIADE_DB`
(la base de l'instance, installée ou `tauri dev`) et `ROBIADE_BIN` (le binaire n'est pas dans le
PATH de l'agent) ; le prompt d'une tâche nomme la commande et dit de ne la lancer qu'à ma demande.
Rien ne ferme une tâche tout seul, et le socket des hooks reste en lecture seule. Ces variables
évitent une erreur, elles n'arrêtent pas un agent : il tourne sous mon utilisateur et peut écrire
dans la base. L'app et la commande écrivent la même ligne sans verrou applicatif : la dernière
écriture l'emporte si je modifie la tâche à l'instant où l'agent la ferme.

## Tickets

- #6 — Tâches : démarrer un agent depuis une tâche perso
- #17 — Tâches : un agent peut marquer terminée la tâche qui l'a lancé
