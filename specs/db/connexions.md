# Connexions DB

**En une phrase** : une connexion désigne ses identifiants (trousseau, Vault, ou clair pour une
base dev locale), affiche la prod en rouge, et n'est en lecture seule que si le serveur le garantit.

## Comportement

- **DB** : un client façon DataGrip, réduit à l'essentiel. Moteurs v1 : **PostgreSQL, MariaDB,
  MongoDB, Elasticsearch**.
  - **Connexions** : nom, moteur, environnement (dev / staging / prod), projet optionnel. Chaque
    champ (URL, hôte, port, base, utilisateur, mot de passe) vient d'une valeur saisie, du
    trousseau ou d'une clé Vault choisie dans la liste des secrets d'un montage. Une URL ou un mot
    de passe ne se saisit en clair que pour une connexion dev vers la machine locale (hôte en clair,
    `localhost`/`127.0.0.1`/`::1`) ; il n'est alors jamais renvoyé à l'interface. La production est rouge partout où elle apparaît
    (badge PROD, ligne, formulaire) et une connexion de prod montre son nom et son hôte résolu
    avant de s'ouvrir.

## Règles

| Moteur | Client Rust | Lecture seule garantie par |
|---|---|---|
| PostgreSQL | `tokio-postgres` | utilisateur en lecture seule **et** `default_transaction_read_only = on` |
| MariaDB | `mysql_async` | utilisateur en lecture seule **et** `SET SESSION TRANSACTION READ ONLY` |
| MongoDB | `mongodb` (officiel) | rôle `read` côté serveur, et c'est tout : le protocole n'a pas de mode lecture seule par session |
| Elasticsearch | `reqwest` (API REST) | clé d'API à privilèges `read` ; et côté adapter, une liste blanche d'endpoints, pas de méthodes (`_search` se fait aussi en `POST`) |

## Décisions

Décidé le 2026-10-28 : **Vault en KV v2 statique, variables choisies à la main.** Les montages
existants (`forum-kv`, `boutique-kv`…) portent les identifiants applicatifs ; il n'y a pas de
secrets dynamiques. Robiade ne fait aucun flux d'authentification : il réutilise le jeton que la
CLI dépose après `vault login -method=oidc` (`~/.vault-token`), ou un jeton de secours au trousseau.
Il liste les noms des secrets d'un montage et je choisis la clé de chaque champ ; les valeurs ne
sont lues qu'au moment de se connecter, en mémoire, et ne remontent jamais à l'interface.
Conséquence : **la prod passe par les identifiants applicatifs**, donc en lecture-écriture côté
serveur. Robiade impose la lecture seule (paramètre de session, vérifié sur le serveur au test),
sans bascule possible en prod : seule la console y écrit, par INSERT, UPDATE ou DELETE validés un
à un (voir la console Postgres). TLS exigé hors dev,
certificat et nom d'hôte vérifiés ; en dev, clair accepté seulement vers localhost. **Risque
résiduel assumé** : ces protections tiennent contre une fausse manipulation, pas contre un bug de
Robiade ; un compte en lecture seule côté serveur reste la cible quand il sera disponible.

Seule exception au « rien en clair » : une base **dev locale**, dont le mot de passe est déjà en
clair dans le `.env` du projet, se saisit en clair et reste dans `robiade.db` ; staging, prod et dev
distant passent par Vault ou le trousseau.
