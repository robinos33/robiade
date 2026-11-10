# Inbox

**En une phrase** : l'écran d'accueil, qui liste par priorité ce qui demande mon action maintenant.

## Comportement

Vue en lecture seule, construite à partir des événements des autres contextes. Elle répond à une
seule question : **qu'est-ce qui demande mon action maintenant ?** Par ordre de priorité :
1. agent en attente de permission ou de réponse ;
2. pipeline rouge sur une de mes MR ;
3. MR des autres en attente de ma review ;
4. discussion non résolue sur une de mes MR ;
5. [routine](routines.md) en échec à son dernier passage, tant que je ne l'ai pas marquée vue. Un
   nouveau passage en échec la fait revenir, un passage réussi l'en sort ;
6. agent terminé dont je n'ai pas lu la fin.

Le [status d'une session](agents/status.md) est la brique de base ; l'Inbox en est l'agrégation.

## Tickets

- #62 — Routines — voir, lancer et créer mes tâches planifiées launchd
