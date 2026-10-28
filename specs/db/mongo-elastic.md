# Mongo et Elastic

**En une phrase** : sans lecture seule de session, la lecture seule de Mongo et d'Elastic est tenue
par une liste blanche de lectures dans la console, et la prod n'y écrit jamais.

## Règles

Conséquence : pour Mongo et Elastic, la lecture seule d'une connexion est tenue par la liste
blanche de la console (voir les décisions ci-dessous), pas par le serveur. Un compte en lecture
seule reste la cible ; le test dit si le compte pourrait écrire (rôles Mongo, `_has_privileges`
d'Elastic).

## Décisions

Décidé le 2026-10-28, avec Vault (voir les [connexions](connexions.md)) : pour Mongo et Elastic, qui n'ont pas de
lecture seule de session, la console de prod sera limitée à une liste blanche de commandes de
lecture.

Décidé le 2026-10-28 : **Elasticsearch, liste blanche de lectures, jamais d'écriture en prod.**
Sans lecture seule de session, une connexion en lecture seule, et toute connexion de prod,
n'envoie que les requêtes d'une liste blanche, jugée sur la méthode et l'endpoint (`_search`,
`_count`, `_mget`, `_sql`, `_cat`, `_mapping`, `_doc` en GET…), jamais sur le corps. Un GET n'est
pas une lecture par principe : `GET _refresh` et `GET _flush` agissent. Le chemin est jugé décodé
(`%5Fdelete_by_query` est `_delete_by_query`) ; un segment indécodable, un `/` encodé, `.` ou `..`
sont refusés partout. Pas d'écriture en prod : sans transaction, rien à valider ou annuler. Hors
prod, le reste part après confirmation. Une page d'index avance par `from`, dans la fenêtre de
10 000 documents. Une recherche dont des shards ont échoué, ou qui a dépassé son délai, est une
erreur, pas un résultat vide. L'annulation retrouve les tâches par `X-Opaque-Id` ; Elasticsearch
ne la constate qu'entre deux étapes de la recherche.
