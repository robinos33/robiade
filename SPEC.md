# Robiade — spécification v0 (brouillon)

*Robiade : Robin + IA + ADE (Agentic Development Environment). Nom de travail.*

> **En une ligne** : l'ADE est un poste de pilotage local pour mes agents Claude Code.
> Il ne possède aucune donnée métier : il lit l'état là où il vit (sessions Claude Code,
> GitLab, Git, Docker, DB), le rend lisible d'un coup d'œil et permet d'agir dessus.
> Tauri 2 (cœur Rust hexagonal + front Svelte), secrets dans le Keychain, aucun port réseau ouvert.

Statut : brouillon du 2026-10-27. Les points marqués **[À trancher]** sont regroupés en §10.

---

## 1. Périmètre

### Dans le périmètre
- **Agents** : sessions Claude Code **lancées par l'ADE** sur cette machine. Les lancer, leur parler
  dans le terminal intégré, voir leur status, consulter leur historique.
- **Forge** : MR, pipelines et issues GitLab (`gitlab.example.com` uniquement). Voir et agir.
  Les issues GitLab sont les **tâches d'équipe** : l'ADE les consomme, GitLab reste la source.
- **Tâches perso** : par projet et globales. **L'ADE en est la source de vérité** — c'est la
  seule donnée métier qu'il possède.
- **Environnements** : worktrees Git, conteneurs Docker, et un client DB (console de requêtes +
  grille de données façon DataGrip) pour PostgreSQL, MariaDB, MongoDB, Elasticsearch.
- **Statistiques** : une page globale, et les stats de la session en cours.
- **Inbox** : ce qui demande mon attention, par projet et global.

### Hors périmètre (explicitement)
- Être la source de vérité de quoi que ce soit, hors tâches perso et préférences d'interface.
- Agents distants, routines cloud, autres machines.
- Sessions Claude Code lancées hors de l'ADE (iTerm, IDE) : ni affichées ni pilotées. Leur
  transcript reste consultable dans l'historique, rien de plus.
- Édition de données dans la grille DB (cellule par cellule) en v1 : voir §4.4.
- Éditeur de code. L'ADE ouvre PhpStorm, il ne le remplace pas.
- Multi-utilisateur, synchronisation, hébergement.

---

## 2. Principe directeur : l'ADE est une vue, pas un système

| Donnée | Source de vérité | Ce que l'ADE en garde |
|---|---|---|
| Sessions vivantes | `~/.claude/sessions/<pid>.json` + hooks Claude Code | Rien (état en mémoire) |
| Historique des sessions | `~/.claude/projects/<projet>/<session>.jsonl` | Index de recherche SQLite, **reconstructible** |
| MR, pipelines, issues | GitLab | Cache court (TTL), jetable |
| Worktrees, branches | Le dépôt Git | Rien |
| Conteneurs | Docker Engine | Rien |
| Connexions DB | Config ADE (hôte, port, user) + Keychain (mot de passe) | La config — c'est la seule donnée propre à l'ADE avec les préférences |
| Statistiques | Dérivées des transcripts | Agrégats SQLite, **reconstructibles** |
| Résultats de requêtes DB | La base interrogée | **Rien sur disque** : en mémoire, le temps de l'affichage |
| Historique des requêtes DB | ADE | SQLite (texte des requêtes, jamais les résultats) |
| **Tâches perso** | **ADE** | SQLite, **sauvegardée** (export JSON horodaté quotidien) |
| Identifiants DB de prod | Vault | Rien : secrets dynamiques à durée de vie courte, en mémoire |
| Préférences, vues sauvegardées | ADE | SQLite |

Conséquence de design : **supprimer le cache de l'ADE ne doit rien faire perdre.** Les tâches perso
sont la seule exception assumée ; elles vivent dans un fichier SQLite distinct du cache
(`robiade.db` contre `cache.db`), sauvegardé, et le cache se reconstruit. C'est un test d'acceptation.

---

## 3. Choix techniques

| Sujet | Choix | Pourquoi |
|---|---|---|
| Shell applicatif | **Tauri 2** | Client lourd, aucun serveur HTTP local, IPC typé, permissions par commande. Voir discussion du 2026-10-27. |
| Front | **Svelte 5** (runes) + TypeScript strict | Réactivité fine sans DOM virtuel : les mises à jour de status à haute fréquence ne re-rendent que la ligne concernée. |
| Async | tokio | Pollers GitLab, watchers fichiers, PTY, Docker en parallèle. |
| Stockage local | SQLite (`rusqlite`), FTS5 pour la recherche d'historique | Un fichier, pas de service. |
| Git | `gix` | Pas de fork de process `git` pour lister worktrees et statuts. |
| Docker | `bollard` (socket Unix) | API officielle, pas de parsing de `docker ps`. |
| GitLab | `ureq` + `PRIVATE-TOKEN` | Règle existante : pas de `glab` en non-interactif (401 intermittents). Client synchrone comme les autres ports ; HTTPS seul, aucune redirection suivie (le token, en-tête maison, la suivrait). |
| Terminal intégré | `portable-pty` côté Rust, xterm.js côté front | Pour héberger le vrai CLI `claude` (§4.1). |
| Secrets | `security-framework` (Keychain macOS) | Jamais en clair sur disque. |

Plateforme cible : **macOS arm64 uniquement** en v1. Pas de portabilité spéculative.

---

## 4. Domaine — contextes délimités

Trois contextes plus un noyau partagé. Le découpage suit les sources de vérité : chaque contexte
correspond à un système externe différent, ce qui rend la frontière évidente. Les tâches étant
des issues GitLab, elles vivent dans Forge : un contexte « Work » séparé n'aurait rien porté
d'autre qu'un renommage.

```
                    ┌──────────────────────┐
                    │  Shared kernel       │
                    │  Project, ProjectId, │
                    │  RepoPath, GitlabRef │
                    └──────────┬───────────┘
        ┌──────────────────┼───────────────────┐
        ▼                  ▼                   ▼
   ┌─────────┐       ┌───────────┐      ┌──────────────┐
   │ Agents  │       │  Forge    │      │ Environments │
   │ (ACL →  │       │ (ACL →    │      │ (ACL → Git,  │
   │ Claude  │       │  GitLab : │      │ Docker, DB)  │
   │ Code)   │       │ MR, issues│      └──────────────┘
   └─────────┘       └───────────┘
        └──────── tous publient vers ──────────┐
                                               ▼
                              ┌──────────────────────────────┐
                              │ Modèles de lecture :         │
                              │ Inbox, Statistiques          │
                              └──────────────────────────────┘
```

Tous les contextes sont des **couches anti-corruption** sur des systèmes externes : le domaine
porte surtout des invariants de lecture et des règles d'action (« peut-on merger ? »), peu
d'état propre. C'est voulu ; pas d'agrégat inventé pour faire du DDD.

### 4.0 Noyau partagé — `Project`
Un `Project` relie un dépôt local à son projet GitLab : `ProjectId`, `RepoPath`, `GitlabProjectRef`,
nom d'affichage. Détecté depuis le remote `origin` ; un dépôt dont le remote n'est pas sur
l'instance configurée reste un projet local sans volet Forge.

### 4.1 Agents
**Langage** : `Agent` (un process `claude` vivant), `Session` (une conversation, identifiée par
`sessionId`, qui survit au process), `AgentStatus`, `PermissionRequest`, `Transcript`.

**Sources observées sur ce poste (Claude Code 2.1.310)** :
- `~/.claude/sessions/<pid>.json` : registre des agents vivants — `pid`, `sessionId`, `cwd`, `name`,
  `status` (observé : `idle`), `statusUpdatedAt`, `kind`, `messagingSocketPath`.
- `~/.claude/projects/<cwd encodé>/<sessionId>.jsonl` : transcript — types `user`, `assistant`,
  `attachment`, `system`, `custom-title`, `last-prompt`, `pr-link` (lien MR !), avec `gitBranch`.
- **Hooks** (`SessionStart`, `PreToolUse`, `PermissionRequest`, `Notification`, `Stop`, `SessionEnd`…) :
  seule interface **documentée**.

**Règle de robustesse** : le registre et le format jsonl sont **internes et non documentés**. Ils
peuvent changer à n'importe quelle version. L'adapter les parse de façon tolérante (champs inconnus
ignorés, champs absents → état « inconnu », jamais de panic), versionne ce qu'il sait lire, et un
test de contrat tourne sur de vrais fichiers anonymisés à chaque montée de version de Claude Code.
Les hooks sont la source primaire d'événements ; le registre sert à la réconciliation au démarrage.

**Piloter un agent** — décidé le 2026-10-27 : **CLI intégré**. L'ADE lance le vrai `claude` dans un
pseudo-terminal affiché par xterm.js. Fidélité totale (même CLI, mêmes skills, mêmes `/commands`) ;
l'état structuré vient des hooks, jamais du parsing de la sortie terminal.
Écartés : Agent SDK / `claude -p --output-format stream-json` (ce n'est plus la session
interactive) ; `messagingSocketPath` (protocole interne).

**Status d'une session** — machine à états, pilotée par les hooks :

| État | Déclencheur | Rendu |
|---|---|---|
| `Running` | `UserPromptSubmit`, `PreToolUse`, `PostToolUse` | animé, neutre |
| `AwaitingPermission` | `PermissionRequest` | **le plus visible** : bloque l'agent |
| `AwaitingInput` | `Stop`, `Notification` (idle) — tour fini, pas encore lu | visible |
| `Idle` | `AwaitingInput` dont j'ai ouvert la session | discret |
| `Failed` | `StopFailure` | rouge |
| `Ended` | `SessionEnd`, ou process disparu | grisé |

Process disparu sans `SessionEnd` (crash, `kill`) : détecté par vérification du `pid`, en comparant
`procStart` pour ne pas se faire piéger par la réutilisation de PID. Au démarrage de l'ADE, l'état
initial vient du registre, puis les hooks prennent le relais.

**Indicateurs portés par chaque ligne de session** (en plus du status) :
- **MR** : pictogramme si une MR est liée (`pr-link` du transcript, sinon branche ↔ MR). La couleur
  donne l'état de la MR : draft gris, ouverte bleu, mergée violet (convention GitLab), fermée rouge.
  Une pastille donne le pipeline : succès, échec, en cours. Le pictogramme change aussi de forme
  selon l'état, pour ne pas reposer sur la couleur seule (daltonisme).
- **Branche et worktree**, avec un marqueur si l'arbre a des modifications non commitées.
- **Mode de permission** (`permissionMode` du transcript) : signalé visuellement quand il est
  permissif (auto, bypass). C'est un indicateur de sécurité, pas de confort.
- **Contexte consommé** (en %) : prévient avant une compaction.
- **Dernière activité** (« il y a 4 min ») et durée de la session.
- **Sous-agents en cours**, s'il y en a.
- Coût et tokens : dans le détail seulement, pas sur la ligne.

**Seules les sessions lancées par l'ADE existent pour lui.** Mécanisme, sans toucher à la config
globale de Claude Code :
- l'ADE génère le `sessionId` et lance `claude --session-id <uuid> --settings <json>` ;
- le JSON passé par `--settings` déclare les hooks de l'ADE, qui n'existent donc que pour ses
  propres sessions. `~/.claude/settings.json` reste intact ;
- un hook qui reçoit un `sessionId` inconnu est ignoré.

Cycle de vie : fermer l'ADE termine ses sessions (confirmation s'il y en a en `Running` ou
`AwaitingPermission`). Rien n'est perdu : au redémarrage, une session se reprend avec
`claude --resume <sessionId>`. Un démon qui garderait les PTY vivants (façon tmux) est écarté en
v1 : c'est un deuxième process à sécuriser pour un gain faible.

**Historique** : liste filtrable par projet, branche, date, MR liée (`pr-link`) ; recherche plein
texte (FTS5) ; reprise d'une session (`claude --resume <sessionId>` dans un PTY).

### 4.2 Forge (GitLab)
**Langage** : `MergeRequest`, `Pipeline`, `Job`, `Discussion`, `Approval`, `MergeTrain` (pour les
wagons empilés).

**Lecture** : mes MR (auteur ou relecteur), état pipeline, discussions non résolues, conflits,
approbations ; MR des autres en attente de ma review.

**Actions** (toutes confirmées dans l'UI, voir §6) : commenter, approuver, rebaser, relancer un job,
passer draft/ready, merger. Règles de domaine portées par le code, pas par l'utilisateur :
- un wagon intermédiaire (cible ≠ `staging`/`master`) a `squash = false` ;
- jamais de merge du parent dans un wagon ;
- ces règles ne s'appliquent qu'à mes MR.

**Issues = tâches** : vue par projet, et vue globale (issues qui me sont assignées, tous projets
confondus). Création, changement d'état, assignation, labels : tout part vers GitLab, l'ADE ne
garde rien. Une issue peut lancer une session (« démarrer un agent sur cette issue » : worktree +
branche + prompt initial pré-rempli avec l'issue, que je relis avant envoi).

Lien Agents ↔ Forge : une session porteuse d'un `pr-link` ou dont la branche correspond à une MR
est affichée **sur** la MR, et inversement.

### 4.2 bis Tâches perso — le seul vrai domaine
C'est le seul contexte où l'ADE porte de l'état, donc le seul où un agrégat a un sens.
- **Agrégat `Task`** : identifiant, titre (non vide, ≤ 200 caractères), notes, projet optionnel
  (absent = tâche globale), statut `Todo → Doing → Done` (retour arrière autorisé, `Done` horodaté),
  échéance optionnelle, liens optionnels vers une session ou une issue GitLab.
- Une tâche perso peut **référencer** une issue d'équipe, jamais la dupliquer : pas de
  synchronisation, pas de champ recopié.
- Vue unifiée : la liste des tâches d'un projet montre les deux types côte à côte, typés
  visuellement ; seules les tâches perso sont éditables en place.

### 4.3 Statistiques (modèle de lecture)
Calculées à partir des transcripts et des événements de hooks, stockées en agrégats SQLite
reconstructibles. Aucune nouvelle source.

**Session en cours** (panneau latéral du terminal) : durée, tokens entrée / sortie / cache,
contexte consommé, coût estimé, outils appelés (par type), fichiers modifiés, sous-agents lancés,
temps passé en `AwaitingPermission`.

**Page globale**, filtrable par projet et par période :
- sessions par jour, durée médiane, taux de `Failed` ;
- tokens et coût estimé par projet et par modèle ;
- outils les plus utilisés ; demandes de permission les plus fréquentes (candidates à une règle
  `allow`, à décider à la main) ;
- **temps où un agent m'a attendu** (`AwaitingPermission` + `AwaitingInput` non lu) : c'est la
  mesure de qui est le goulot, l'agent ou moi ;
- MR produites par des sessions, et part de celles qui ont été mergées.

**Activité GitLab** (mes contributions, par jour / semaine / mois / année) : MR ouvertes, MR
mergées, commits, reviews faites. Source : API GitLab (`events`, `merge_requests`, `commits` par
projet), agrégée localement et mise en cache ; le cache se reconstruit.

Le coût est une **estimation** : tokens × grille tarifaire versionnée dans l'ADE. Avec un
abonnement, ce n'est pas ce que je paie. L'UI met les tokens en premier et le coût en second.

### 4.4 Environments
- **Worktrees** : liste par projet, branche, propreté (fichiers modifiés), agent actif dessus,
  création et suppression (suppression refusée si modifications non commitées ou agent actif).
- **Docker** : conteneurs par projet (label compose `com.docker.compose.project`), état, ports,
  start/stop/restart, logs en flux.
- **DB** : un client façon DataGrip, réduit à l'essentiel. Moteurs v1 : **PostgreSQL, MariaDB,
  MongoDB, Elasticsearch**.
  - **Explorateur** : connexions → bases → schémas → tables / collections / index.
  - **Grille** : ouvrir une table affiche ses lignes dans une grille virtualisée, triable et
    filtrable. Pagination côté serveur (keyset quand une clé le permet, sinon `LIMIT/OFFSET`) :
    on ne charge jamais une table entière. Valeurs JSON et longues dans un panneau de détail.
  - **Console** : éditeur CodeMirror 6 avec coloration par dialecte, exécution de la requête sous
    le curseur, résultat dans la même grille, historique local, **annulation** d'une requête
    longue (`pg_cancel_backend`, `KILL QUERY`, `killOp`, annulation de tâche Elastic) et délai
    maximal par connexion.
  - **Langage de la console** : SQL pour Postgres et MariaDB. Pour Elastic, le format de la Dev
    Tools de Kibana (`GET /index/_search` + corps JSON). Pour Mongo, des commandes JSON
    (`find`, `aggregate` avec filtre et pipeline en JSON), **pas** de syntaxe `mongosh` : ce serait
    embarquer un interpréteur JavaScript, donc une surface d'exécution de code en plus.
  - Mongo et Elastic n'ont pas de tables : la grille aplatit les documents sur leurs champs de
    premier niveau, et le reste s'ouvre en arbre JSON.
  - **Pas d'édition de cellule en v1.** Une édition en grille est une écriture déguisée, avec un
    ciblage (clé primaire, documents sans clé stable) facile à rater. Les écritures passent par
    la console, sur une connexion en mode écriture, avec confirmation.

  | Moteur | Client Rust | Lecture seule garantie par |
  |---|---|---|
  | PostgreSQL | `tokio-postgres` | utilisateur en lecture seule **et** `default_transaction_read_only = on` |
  | MariaDB | `mysql_async` | utilisateur en lecture seule **et** `SET SESSION TRANSACTION READ ONLY` |
  | MongoDB | `mongodb` (officiel) | rôle `read` côté serveur, et c'est tout : le protocole n'a pas de mode lecture seule par session |
  | Elasticsearch | `reqwest` (API REST) | clé d'API à privilèges `read` ; et côté adapter, une liste blanche d'endpoints, pas de méthodes (`_search` se fait aussi en `POST`) |

  Conséquence : pour Mongo et Elastic, la protection côté client est faible. **Une connexion
  marquée lecture seule exige des identifiants en lecture seule**, l'ADE ne compense pas un compte
  trop privilégié. Il l'affiche en revanche quand il le détecte (rôles Mongo, privilèges de la clé
  Elastic via `_security/_authenticate`).

### 4.5 Status (agrégation)
Vue en lecture seule, construite à partir des événements des autres contextes. Elle répond à une
seule question : **qu'est-ce qui demande mon action maintenant ?** Par ordre de priorité :
1. agent en attente de permission ou de réponse ;
2. pipeline rouge sur une de mes MR ;
3. MR des autres en attente de ma review ;
4. discussion non résolue sur une de mes MR ;
5. agent terminé dont je n'ai pas lu la fin.

Aussi exposé hors fenêtre : badge du Dock et notification macOS sur les niveaux 1 et 2.
Le status d'une session (§4.1) est la brique de base ; l'Inbox en est l'agrégation.

---

## 5. Architecture du code

Hexagonal, avec le moins de crates possible. La règle de dépendance est **vérifiée par le
compilateur** : le crate `core` n'a pas accès aux crates d'I/O, donc il ne peut pas les appeler.

```
ade/
├── Cargo.toml                  # workspace
├── crates/
│   ├── core/                   # domaine + cas d'usage, un module par contexte
│   │   └── src/{project,agents,forge,environments,inbox,stats}/
│   │       ├── domain.rs       # entités, value objects, règles
│   │       ├── ports.rs        # traits (SessionRegistry, ForgeGateway, …)
│   │       └── usecases.rs
│   ├── adapters/               # implémentations des ports : claude_code, gitlab,
│   │                           # git, docker, db, keychain, sqlite
│   └── app/                    # Tauri : composition root, commandes IPC, events
└── ui/                         # Svelte 5
```

- `core/Cargo.toml` : dépendances autorisées = `thiserror`, `time`, `uuid`. Pas de tokio, serde,
  reqwest. Les ports async utilisent les `async fn` dans les traits (Rust stable).
- Un crate de plus seulement quand un contexte le justifie (temps de compilation, isolation) —
  pas avant.
- Tests : domaine en tests unitaires purs ; adapters en tests de contrat sur fixtures réelles ;
  bout en bout à la main en v0.
- Le front ne connaît que des **DTO de lecture** et des **commandes** ; aucune logique métier en
  TypeScript. Types générés depuis Rust (`specta` / `tauri-specta`) pour éviter la dérive.

---

## 6. Sécurité

L'ADE concentre un token GitLab, des accès DB et le socket Docker : c'est une cible de valeur.
Le niveau d'exigence est celui d'un outil d'administration.

**Surface d'attaque**
- **Aucun port réseau en écoute.** Le canal hooks → ADE est un socket Unix dans
  `~/Library/Application Support/ade/`, permissions `0600`, et l'ADE vérifie l'UID du pair
  (`getpeereid`).
- Sorties réseau **sur liste blanche** : `gitlab.example.com` et les hôtes DB déclarés.
- Tauri : capabilities minimales commande par commande ; pas de plugin `shell` ni `fs` exposé au
  front ; CSP stricte (`default-src 'self'`, pas d'`unsafe-inline`, pas d'`unsafe-eval`) ; aucun
  contenu distant chargé dans la WebView.

**Secrets**
- Keychain uniquement (token GitLab, mots de passe DB). Jamais dans SQLite, les logs, les messages
  d'erreur ou l'IPC vers le front.
- Type `Secret<T>` (`secrecy`) côté Rust : pas de `Debug`/`Display`, effacé en mémoire au drop.

**Données non fiables**
- Tout ce qui vient des transcripts, de GitLab (titres, descriptions, commentaires, logs de jobs)
  et de Docker est **non fiable** : rendu comme texte, jamais en `{@html}`. Markdown et HTML rendus
  dans une iframe `srcdoc` sandboxée (ni script, ni origine, ni navigation hors de l'app) : aucun
  nettoyage à maintenir, le contenu ne peut rien exécuter.
- Un transcript peut contenir des secrets collés par erreur : la recherche plein texte est locale
  et son index SQLite vit dans le cache de l'app, exclu de Time Machine (`tmutil addexclusion`).

**Actions**
- Toute action sortante (GitLab, DB en écriture, suppression de worktree, arrêt de conteneur)
  passe par une confirmation qui montre **la cible exacte** (projet, MR, branche). Pas de
  confirmation générique « êtes-vous sûr ? ».
- DB : connexion en lecture seule par défaut ; l'écriture se fait avec un autre utilisateur ou
  une bascule explicite, par connexion. **Prod uniquement via Vault** (§10).
- Journal d'audit local des actions sortantes (quoi, quand, cible, résultat), sans les secrets.

**Chaîne de build**
- `cargo deny` (licences, advisories), `cargo audit`, `npm audit` en CI ; lockfiles commités.
- Passes de mes skills `xss-audit` sur `ui/` à chaque jalon (la WebView est le maillon le plus
  exposé) ; `csrf-safe-method-audit` sans objet tant qu'il n'y a pas de HTTP.

---

## 7. UX

**Principes**
- **Clavier d'abord.** Palette `⌘K` pour tout (aller à un projet, une MR, une session ; lancer
  une action). Chaque action fréquente a un raccourci ; la souris reste optionnelle.
- **Densité maîtrisée.** Une ligne = un objet, avec l'état lisible sans survol. Pas de cartes
  aérées façon dashboard marketing.
- **L'état d'abord, la décoration ensuite.** Le seul code couleur fort est réservé à ce qui
  demande une action (§4.5). Le reste est neutre.
- **Rien ne saute.** Les mises à jour en direct ne déplacent pas la ligne sous le curseur ; le
  tri ne change pas pendant qu'on lit.
- Thème clair et sombre suivant le système ; respect de « Réduire les animations ».

**Structure de l'écran**
```
┌─────────┬──────────────────────────────────────────┬─────────────────┐
│ Inbox   │  Vue courante (Agents | MR | Issues |    │  Détail         │
│ ─────── │  Environnements | DB | Stats)            │  (objet         │
│ Global  │                                          │   sélectionné,  │
│ ─────── │  liste virtualisée, filtrable            │   terminal      │
│ Projets │                                          │   d'agent,      │
│  ade    │                                          │   diff, logs)   │
│  shop   │                                          │                 │
│  forum  │                                          │                 │
└─────────┴──────────────────────────────────────────┴─────────────────┘
```
- **Inbox** = la vue Status (§4.5). C'est l'écran d'accueil.
- Sélectionner un projet filtre toutes les vues ; « Global » enlève le filtre.
- Le panneau de détail d'un agent héberge son terminal ; on peut le détacher dans une fenêtre.

**Budgets de performance** (mesurés, pas supposés)
| Mesure | Cible |
|---|---|
| Démarrage à froid jusqu'à l'Inbox affichée | < 500 ms |
| RAM au repos, 10 sessions suivies | < 200 Mo (process Rust + WebView) |
| Latence d'un événement de hook jusqu'à l'écran | < 100 ms |
| Scroll d'une liste de 5 000 éléments | 60 i/s sans saccade |
| Recherche plein texte dans l'historique | < 150 ms |

---

## 8. Jalons

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

---

## 9. Risques

| Risque | Impact | Parade |
|---|---|---|
| Format interne de Claude Code qui change | v0 cassée à une mise à jour | Hooks en source primaire, parsing tolérant, tests de contrat sur fixtures |
| WebView = Safari (WKWebView) | Quelques API web absentes, debug moins confortable | Cible unique macOS, pas de dépendance aux API Chromium |
| Dérive vers un « tout-en-un » | Projet jamais fini | Hors périmètre §1 tenu ; chaque jalon a un critère de sortie |
| Secret affiché dans un transcript | Fuite par capture d'écran ou partage | Masquage heuristique des motifs de tokens connus à l'affichage (v1) |

---

## 10. À trancher

1. **Vault** : quelle méthode d'authentification (OIDC, token, AppRole) et quels chemins
   (`database/creds/<rôle>` en secrets dynamiques, ou KV statique) ? Les secrets dynamiques sont
   la bonne cible : identifiants à durée de vie courte, révoqués à la fermeture de la connexion,
   et la lecture seule garantie par le rôle Vault, pas par l'ADE. Une connexion prod est marquée
   de façon impossible à confondre (couleur de fenêtre, bandeau permanent).

Décidé le 2026-10-27 : pilotage par CLI intégré (PTY) ; seules les sessions lancées par l'ADE
sont vivantes dans l'ADE ; status par session (§4.1) ; tâches = issues GitLab ; client DB avec
console et grille sur PostgreSQL, MariaDB, MongoDB, Elasticsearch ; page de stats + stats de la
session en cours ; deux types de tâches (perso dans l'ADE, équipe dans GitLab) ; stats GitLab ;
historique antérieur à l'ADE consultable en lecture seule ; prod accessible via Vault.

Décidé le 2026-10-27 (après coup) : **contexte consommé et quotas lus par la statusline.** Robiade
déclare sa propre `statusLine` dans le `--settings` de ses agents ; `robiade statusline` relaie le
JSON sur le socket des hooks et n'écrit rien dans le terminal. L'interface dessine une barre native
(modèle, contexte, quotas 5 h et semaine, seuils 50 % / 80 %). La statusline de `~/.claude` reste
celle d'iTerm. Les champs lus viennent de la statusline existante, pas d'un contrat documenté :
même règle de tolérance que pour les transcripts.
