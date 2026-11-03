# Console MariaDB

**En une phrase** : les règles de la console Postgres, adaptées à ce qui lève la lecture seule
d'une session MariaDB.

## Décisions

Décidé le 2026-10-28 : **MariaDB, mêmes règles, adaptées au dialecte.** Constaté sur MariaDB 11.4 :
`SET SESSION TRANSACTION READ ONLY` refuse DML, DDL, GRANT, tables temporaires et séquences, mais
se lève par `SET tx_read_only = 0`, `SET STATEMENT tx_read_only = 0 FOR …` (la ligne est écrite),
`EXECUTE IMMEDIATE`, `PREPARE` puis `EXECUTE`, et les commentaires exécutables `/*! … */`. Hors
écriture, la console MariaDB refuse donc **tout** `SET` (il change aussi `sql_mode` ou le jeu de
caractères, donc la lecture du texte), le SQL dynamique, `CALL`, `DO`, `BINLOG` et tout commentaire
exécutable ; `XA`, `LOCK`/`UNLOCK TABLES` et `SET autocommit` comptent comme contrôle de
transaction. La session impose un `sql_mode` sans `ANSI_QUOTES` ni `NO_BACKSLASH_ESCAPES` (ni les
modes composés qui les incluent), relu à l'ouverture : la console découpe le texte comme le serveur
le lit. En prod, mêmes INSERT, UPDATE ou DELETE validés un à un ; `REPLACE`, `LOAD DATA`,
`SELECT … INTO OUTFILE` refusés ; fonctions refusées : `LOAD_FILE`, verrous nommés, `BENCHMARK`,
séquences (`NEXTVAL`, `SETVAL`, `NEXT VALUE FOR`), attente de réplication, `sys_exec`/`sys_eval`.
L'écriture part dans `START TRANSACTION READ WRITE`, et l'instruction n'est préparée qu'ensuite :
MariaDB refuse de préparer une écriture dans une session en lecture seule. Même risque résiduel
que pour Postgres.
