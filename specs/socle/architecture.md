# Architecture

**En une phrase** : les choix techniques, le découpage du domaine en contextes délimités et
l'organisation hexagonale du code.

## Comportement

### Choix techniques

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
| Secrets | `security-framework` (Keychain macOS) | Jamais en clair sur disque, sauf le mot de passe d’une base dev locale (§4.4), déjà en clair dans le `.env` du projet. |

Plateforme cible : **macOS arm64 uniquement** en v1. Pas de portabilité spéculative.

### Domaine — contextes délimités

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

### Noyau partagé — `Project`

Un `Project` relie un dépôt local à son projet GitLab : `ProjectId`, `RepoPath`, `GitlabProjectRef`,
nom d'affichage. Détecté depuis le remote `origin` ; un dépôt dont le remote n'est pas sur
l'instance configurée reste un projet local sans volet Forge.

### Agents : langage et sources

**Langage** : `Agent` (un process `claude` vivant), `Session` (une conversation, identifiée par
`sessionId`, qui survit au process), `AgentStatus`, `PermissionRequest`, `Transcript`.

**Sources observées sur ce poste (Claude Code 2.1.310)** :
- `~/.claude/sessions/<pid>.json` : registre des agents vivants — `pid`, `sessionId`, `cwd`, `name`,
  `status` (observé : `idle`), `statusUpdatedAt`, `kind`, `messagingSocketPath`.
- `~/.claude/projects/<cwd encodé>/<sessionId>.jsonl` : transcript — types `user`, `assistant`,
  `attachment`, `system`, `custom-title`, `last-prompt`, `pr-link` (lien MR !), avec `gitBranch`.
- **Hooks** (`SessionStart`, `PreToolUse`, `PermissionRequest`, `Notification`, `Stop`, `SessionEnd`…) :
  seule interface **documentée**.

### Forge : langage

**Langage** : `MergeRequest`, `Pipeline`, `Job`, `Discussion`, `Approval`, `MergeTrain` (pour les
wagons empilés).

### Architecture du code

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

## Règles

**Règle de robustesse** : le registre et le format jsonl sont **internes et non documentés**. Ils
peuvent changer à n'importe quelle version. L'adapter les parse de façon tolérante (champs inconnus
ignorés, champs absents → état « inconnu », jamais de panic), versionne ce qu'il sait lire, et un
test de contrat tourne sur de vrais fichiers anonymisés à chaque montée de version de Claude Code.
Les hooks sont la source primaire d'événements ; le registre sert à la réconciliation au démarrage.
