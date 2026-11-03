# Lancement d'un agent

**En une phrase** : l'ADE lance le vrai `claude` dans un pseudo-terminal, ne connaît que les
sessions qu'il a lancées, et les termine quand il se ferme.

## Comportement

**Seules les sessions lancées par l'ADE existent pour lui.** Mécanisme, sans toucher à la config
globale de Claude Code :
- l'ADE génère le `sessionId` et lance `claude --session-id <uuid> --settings <json>` ;
- le JSON passé par `--settings` déclare les hooks de l'ADE, qui n'existent donc que pour ses
  propres sessions. `~/.claude/settings.json` reste intact ;
- un hook qui reçoit un `sessionId` inconnu est ignoré.
- l'agent ne reçoit aucune variable `CLAUDE*` de l'environnement de l'ADE (issue #16) : un ADE lancé
  depuis une session Claude Code les transmettrait, et `claude` se croirait sous-agent, sans
  transcript ni reprise possible.

Cycle de vie : fermer l'ADE termine ses sessions (confirmation s'il y en a en `Running` ou
`AwaitingPermission`). Rien n'est perdu : au redémarrage, une session se reprend avec
`claude --resume <sessionId>`.

## Décisions

**Piloter un agent** — décidé le 2026-10-27 : **CLI intégré**. L'ADE lance le vrai `claude` dans un
pseudo-terminal affiché par xterm.js. Fidélité totale (même CLI, mêmes skills, mêmes `/commands`) ;
l'état structuré vient des hooks, jamais du parsing de la sortie terminal.

## Écarté

- Écartés : Agent SDK / `claude -p --output-format stream-json` (ce n'est plus la session
  interactive) ; `messagingSocketPath` (protocole interne).
- Un démon qui garderait les PTY vivants (façon tmux) est écarté en
  v1 : c'est un deuxième process à sécuriser pour un gain faible.

## Tickets

- #14 — Agents : confirmer la fermeture de l'app dès qu'un agent est ouvert
- #16 — Agents : ne pas transmettre l'environnement Claude Code hérité au lancement d'un agent
