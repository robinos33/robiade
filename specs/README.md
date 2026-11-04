# Robiade — spécification v0 (brouillon)

*Robiade : Robin + IA + ADE (Agentic Development Environment). Nom de travail.*

> **En une ligne** : l'ADE est un poste de pilotage local pour mes agents Claude Code.
> Il ne possède aucune donnée métier : il lit l'état là où il vit (sessions Claude Code,
> GitLab, Git, Docker, DB), le rend lisible d'un coup d'œil et permet d'agir dessus.
> Tauri 2 (cœur Rust hexagonal + front Svelte), secrets dans le Keychain, aucun port réseau ouvert.

Statut : brouillon du 2026-10-27. Une question ouverte est marquée **[À trancher]** dans la
feature qu'elle concerne ; `grep -rn 'À trancher' specs` les liste.

## Index des features

Un fichier par feature : ce qu'on peut nommer dans l'app et qu'un ticket modifie. Chaque fichier
suit le même plan : « En une phrase », « Comportement », « Règles », « Décisions », « Écarté »,
« Tickets » ; une section vide est omise.

| Fichier | Feature |
|---|---|
| [socle/architecture.md](socle/architecture.md) | Choix techniques, contextes délimités, noyau `Project`, organisation du code |
| [socle/securite.md](socle/securite.md) | Principes de sécurité transverses |
| [socle/ux.md](socle/ux.md) | Principes d'interface, structure de l'écran, budgets de performance |
| [socle/mise-a-jour.md](socle/mise-a-jour.md) | Mise à jour de l'app depuis `origin/main` au lancement |
| [socle/reglages.md](socle/reglages.md) | Page Réglages : instance de forge, Vault, branches de tronc par projet |
| [agents/lancement.md](agents/lancement.md) | Lancer `claude` dans un PTY, sessions de l'ADE seules, cycle de vie |
| [agents/status.md](agents/status.md) | Machine à états d'une session, indicateurs de sa ligne |
| [agents/statusline.md](agents/statusline.md) | Contexte consommé et quotas lus par la statusline |
| [agents/historique.md](agents/historique.md) | Liste, recherche et reprise des sessions passées |
| [agents/notifications.md](agents/notifications.md) | Notification macOS d'un agent, badge du Dock |
| [forge/merge-requests.md](forge/merge-requests.md) | Lecture des MR, actions confirmées, règles des wagons |
| [forge/issues.md](forge/issues.md) | Issues d'équipe, démarrer un agent sur une issue |
| [taches.md](taches.md) | Tâches perso : agrégat, échéance, liens, démarrer un agent, fermeture par l'agent |
| [inbox.md](inbox.md) | Ce qui demande mon action maintenant |
| [stats/session-en-cours.md](stats/session-en-cours.md) | Stats de la session en cours |
| [stats/page-globale.md](stats/page-globale.md) | Page de stats globale, coût estimé |
| [stats/activite-gitlab.md](stats/activite-gitlab.md) | Mes contributions sur la forge dans le temps |
| [environnements/worktrees.md](environnements/worktrees.md) | Worktrees par projet |
| [environnements/docker.md](environnements/docker.md) | Conteneurs par projet |
| [db/connexions.md](db/connexions.md) | Connexions DB : saisie, trousseau, Vault, lecture seule par moteur |
| [db/explorateur-grille.md](db/explorateur-grille.md) | Explorateur et grille de données |
| [db/console.md](db/console.md) | Console de requêtes : règles communes, langage par moteur |
| [db/postgres.md](db/postgres.md) | Console Postgres : lecture seule de session, écriture en prod |
| [db/mariadb.md](db/mariadb.md) | Console MariaDB : mêmes règles, adaptées au dialecte |
| [db/mongo-elastic.md](db/mongo-elastic.md) | Mongo et Elastic, sans lecture seule de session |
| [code.md](code.md) | Onglet Code, visualiseur de fichiers en lecture seule |

## Périmètre

### Dans le périmètre
- **Agents** : sessions Claude Code **lancées par l'ADE** sur cette machine. Les lancer, leur parler
  dans le terminal intégré, voir leur status, consulter leur historique.
- **Forge** : MR, pipelines et issues, sur GitLab ou GitHub, une instance réglée ([réglages](socle/reglages.md)). Voir et agir.
  Les issues sont les **tâches d'équipe** : l'ADE les consomme, la forge reste la source.
- **Tâches perso** : par projet et globales. **L'ADE en est la source de vérité** — c'est la
  seule donnée métier qu'il possède.
- **Environnements** : worktrees Git, conteneurs Docker, et un client DB (console de requêtes +
  grille de données façon DataGrip) pour PostgreSQL, MariaDB, MongoDB, Elasticsearch.
- **Statistiques** : une page globale, et les stats de la session en cours.
- **Inbox** : ce qui demande mon attention, par projet et global.
- **Code** : parcourir et lire les fichiers du projet ou du worktree sélectionné, en lecture
  seule, pour un coup d'œil sans ouvrir PhpStorm.

### Hors périmètre (explicitement)
- Être la source de vérité de quoi que ce soit, hors tâches perso et préférences d'interface.
- Agents distants, routines cloud, autres machines.
- Sessions Claude Code lancées hors de l'ADE (iTerm, IDE) : ni affichées ni pilotées. Leur
  transcript reste consultable dans l'historique, rien de plus.
- Édition de données dans la grille DB (cellule par cellule) en v1 : voir [explorateur et grille](db/explorateur-grille.md).
- Éditeur de code. L'onglet Code lit, il n'édite pas (ni écriture, ni LSP) : l'ADE ouvre
  PhpStorm, il ne le remplace pas.
- Multi-utilisateur, synchronisation, hébergement.

## Principe directeur : l'ADE est une vue, pas un système

| Donnée | Source de vérité | Ce que l'ADE en garde |
|---|---|---|
| Sessions vivantes | `~/.claude/sessions/<pid>.json` + hooks Claude Code | Rien (état en mémoire) |
| Historique des sessions | `~/.claude/projects/<projet>/<session>.jsonl` | Index de recherche SQLite, **reconstructible** |
| MR, pipelines, issues | GitLab | Cache court (TTL), jetable |
| Worktrees, branches | Le dépôt Git | Rien |
| Conteneurs | Docker Engine | Rien |
| Connexions DB | Config ADE : chaque champ en valeur, référence trousseau ou référence Vault | La config, **sans valeur secrète** : une URL ou un mot de passe se désigne, il ne se saisit pas — sauf pour une base dev locale |
| Statistiques | Dérivées des transcripts | Agrégats SQLite, **reconstructibles** |
| Résultats de requêtes DB | La base interrogée | **Rien sur disque** : en mémoire, le temps de l'affichage |
| Historique des requêtes DB | ADE | SQLite (texte des requêtes, jamais les résultats) |
| **Tâches perso** | **ADE** | SQLite, **sauvegardée** (export JSON horodaté quotidien, `backups/tasks-AAAA-MM-JJ.json` à côté de la base, 30 gardés ; restauration à la main) |
| Identifiants DB de prod | Vault (KV v2 statique) | Rien : valeur lue au moment de se connecter, en mémoire |
| Préférences, vues sauvegardées | ADE | SQLite |

Conséquence de design : **supprimer le cache de l'ADE ne doit rien faire perdre.** Les tâches perso
sont la seule exception assumée ; elles vivent dans un fichier SQLite distinct du cache
(`robiade.db` contre `cache.db`), sauvegardé, et le cache se reconstruit. C'est un test d'acceptation.

## Jalons

| Jalon | Contenu | Critère de sortie |
|---|---|---|
| **v0 — Agents** | Squelette hexagonal, lancement de `claude` dans un PTY, hooks via `--settings`, status par session, Inbox minimale | Je fais une journée de travail avec les agents depuis l'ADE, sans iTerm |
| **v1 — Mémoire** | Historique et recherche, reprise de session, stats de la session en cours, page de stats | Je retrouve n'importe quelle session passée en moins de 10 s |
| **v2 — Forge** | MR, pipelines, reviews en attente, issues par projet et globales, lien session ↔ MR, actions confirmées | Plus besoin d'ouvrir GitLab pour le suivi courant |
| **v3 — Environnements** | Worktrees, Docker | — |
| **v4 — DB** | Explorateur, grille, console, sur les 4 moteurs | Je n'ouvre plus DataGrip pour lire des données |

Ordre justifié par la valeur : les agents sont le cœur de l'outil, et c'est la partie dont les
sources sont les plus fragiles (formats internes). Mieux vaut en découvrir les limites en premier.
Le client DB arrive en dernier parce que c'est **le plus gros morceau** (4 moteurs, 3 langages de
requête, une grille performante) et le moins lié aux agents ; DataGrip fait le travail en attendant.

## Risques

| Risque | Impact | Parade |
|---|---|---|
| Format interne de Claude Code qui change | v0 cassée à une mise à jour | Hooks en source primaire, parsing tolérant, tests de contrat sur fixtures |
| WebView = Safari (WKWebView) | Quelques API web absentes, debug moins confortable | Cible unique macOS, pas de dépendance aux API Chromium |
| Dérive vers un « tout-en-un » | Projet jamais fini | [Hors périmètre](#hors-périmètre-explicitement) tenu ; chaque jalon a un critère de sortie |
| Secret affiché dans un transcript | Fuite par capture d'écran ou partage | Masquage heuristique des motifs de tokens connus à l'affichage (v1) : préfixes connus (GitLab, Anthropic, GitHub, Slack, AWS, Vault, JWT, `Bearer`) et blocs de clé privée, masqués avant l'IPC dans l'historique et la recherche. Un mot de passe quelconque passe ; le terminal d'un agent vivant n'est pas masqué |
