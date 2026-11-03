# Inbox

**En une phrase** : l'écran d'accueil, qui liste par priorité ce qui demande mon action maintenant.

## Comportement

Vue en lecture seule, construite à partir des événements des autres contextes. Elle répond à une
seule question : **qu'est-ce qui demande mon action maintenant ?** Par ordre de priorité :
1. agent en attente de permission ou de réponse ;
2. pipeline rouge sur une de mes MR ;
3. MR des autres en attente de ma review ;
4. discussion non résolue sur une de mes MR ;
5. agent terminé dont je n'ai pas lu la fin.

Le status d'une session (§4.1) est la brique de base ; l'Inbox en est l'agrégation.
