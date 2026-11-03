# Console DB

**En une phrase** : exécuter une instruction à la fois dans le langage du moteur, avec un
résultat borné, une annulation, et des écritures confirmées une à une.

## Comportement

- **Console** : éditeur CodeMirror 6 avec coloration par dialecte, exécution de la requête sous
  le curseur, résultat dans la même grille, historique local, **annulation** d'une requête
  longue (`pg_cancel_backend`, `KILL QUERY`, `killOp`, annulation de tâche Elastic) et délai
  maximal par connexion. Une instruction à la fois, validée aussitôt (pas de `BEGIN` dans la
  console), résultat borné à 1000 lignes ; hors lecture seule, toute écriture est confirmée sur
  l'instruction exacte et la cible. **En prod** (décision du 2026-10-28, détaillée pour [Postgres](postgres.md) et [MariaDB](mariadb.md)) : lectures, et
  INSERT, UPDATE ou DELETE seulement — UPDATE et DELETE avec un WHERE au niveau principal —,
  sans fonction d'administration connue ; chacun est confirmé en tapant le nom de la
  connexion, exécuté par Robiade dans une transaction qu'il ouvre lui-même, puis validé ou
  annulé une fois le nombre de lignes affiché, annulé d'office au bout de 60 s ou si la session
  se ferme, et inscrit au journal d'audit. L'explorateur et la grille restent en lecture seule.
- **Langage de la console** : SQL pour Postgres et MariaDB. Pour Elastic, le format de la Dev
  Tools de Kibana (`GET /index/_search` + corps JSON). Pour Mongo, des commandes JSON
  (`find`, `aggregate` avec filtre et pipeline en JSON), **pas** de syntaxe `mongosh` : ce serait
  embarquer un interpréteur JavaScript, donc une surface d'exécution de code en plus.
