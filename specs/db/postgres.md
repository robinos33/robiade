# Console Postgres

**En une phrase** : la console Postgres refuse ce qui lèverait la lecture seule de session, et en
prod n'écrit que par INSERT, UPDATE ou DELETE validés un à un.

## Décisions

Décidé le 2026-10-28, avec Vault (voir les connexions) : Robiade impose la lecture seule par un
paramètre de session. Ce paramètre arrête l'écriture accidentelle, mais le serveur accepte
qu'une instruction de la session le désactive (`SET default_transaction_read_only = off`, constaté
le 2026-10-28) : la console Postgres refuse donc avant envoi ce qui lève le verrou (`SET`/`RESET` de
la lecture seule, `SET ROLE`, `SESSION AUTHORIZATION`, `set_config`, `RESET ALL`, `DISCARD ALL`,
`DO`, `CALL`, `COPY … PROGRAM`, tout contrôle de transaction), relit l'état après chaque
instruction et ferme la session s'il a changé. Une fonction côté serveur qui lèverait le verrou
échappe à cette analyse.

Décidé le 2026-10-28 : **en prod,
la console lit, et écrit par INSERT, UPDATE ou DELETE validés un à un ; rien d'autre.** MERGE,
COPY, TRUNCATE, DDL, droits, VACUUM, LOCK, `EXPLAIN ANALYZE` d'une écriture et les CTE qui écrivent
sont refusés ; un UPDATE ou un DELETE sans WHERE au niveau principal aussi. Les fonctions
d'administration ou à effet de bord connues (`pg_terminate_backend`, `set_config`, `nextval`,
`lo_*`, `dblink*`, `pg_advisory*`, `pg_read_file`…) sont refusées partout, même dans un SELECT.
L'écriture part dans une transaction `BEGIN TRANSACTION READ WRITE` ouverte par Robiade, pas par
le texte saisi : la session reste en lecture seule pour tout le reste, et l'est de nouveau,
vérifié sur le serveur, après le COMMIT ou le ROLLBACK. Tant qu'une modification attend, la
session refuse toute autre requête. **Risque résiduel** : le repérage des fonctions est lexical,
une fonction maison qui écrit passe ; `WHERE true` passe aussi (c'est le nombre de lignes affiché
avant validation qui le rattrape) ; et l'écriture se fait avec les identifiants applicatifs, donc
avec tous leurs droits sur ce que l'instruction touche.
