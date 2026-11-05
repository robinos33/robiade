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
- l'agent reçoit `LC_CTYPE=UTF-8` quand l'environnement de l'ADE ne définit aucune de `LC_ALL`,
  `LC_CTYPE`, `LANG` (issue #48) : c'est le cas d'un ADE lancé depuis le Dock, et la copie de
  Claude Code casserait alors les accents. Une locale héritée n'est jamais écrasée.

Cycle de vie : fermer l'ADE termine ses sessions (confirmation s'il y en a en `Running` ou
`AwaitingPermission`). Rien n'est perdu : au redémarrage, une session se reprend avec
`claude --resume <sessionId>`.

**Session en cours.** Dans le même process, un `/clear` ouvre une nouvelle session Claude Code et
un `/resume` en rejoint une autre ; les hooks rapportent l'une comme l'autre (constaté sur Claude
Code 2.1.318). La session en cours d'un agent est celle du dernier hook reçu. C'est elle que
« Reprendre », « Redémarrer » et la réouverture au démarrage relancent, et c'est à elle que
l'historique rattache l'onglet et son nom. Elle est gardée avec l'onglet d'une exécution de
Robiade à l'autre. La session d'avant un `/clear` reste dans l'historique et dans « Récents », sans
nom, et se reprend comme une autre.

## Règles

- La session rapportée par un `SessionEnd` ne devient jamais la session en cours : c'est celle qui
  se termine, et après un `/clear` son hook peut arriver après le `SessionStart` de la suivante.

## Décisions

**Piloter un agent** — décidé le 2026-10-27 : **CLI intégré**. L'ADE lance le vrai `claude` dans un
pseudo-terminal affiché par xterm.js. Fidélité totale (même CLI, mêmes skills, mêmes `/commands`) ;
l'état structuré vient des hooks, jamais du parsing de la sortie terminal.

Décidé le 2026-11-05 : **après un `/clear`, la session d'avant redevient anonyme** dans l'historique
(issue #44). Le nom de l'onglet suit la session en cours ; le garder aussi sur les sessions
précédentes obligerait à enregistrer toutes les sessions de l'onglet.

Décidé le 2026-11-06 : **`LC_CTYPE` seul, pas `LANG`**, pour l'agent sans locale (issue #48).
L'encodage passe en UTF-8 sans changer la langue des messages ni l'ordre de tri des outils que
l'agent lance, et sans supposer la langue de la machine.

## Écarté

- Écartés : Agent SDK / `claude -p --output-format stream-json` (ce n'est plus la session
  interactive) ; `messagingSocketPath` (protocole interne).
- Un démon qui garderait les PTY vivants (façon tmux) est écarté en
  v1 : c'est un deuxième process à sécuriser pour un gain faible.

## Tickets

- #14 — Agents : confirmer la fermeture de l'app dès qu'un agent est ouvert
- #16 — Agents : ne pas transmettre l'environnement Claude Code hérité au lancement d'un agent
- #48 — Agents — copier une réponse casse les accents : locale UTF-8 absente hors terminal
- #44 — Agents — après un /clear, « Reprendre » et « Redémarrer » relancent la session d'avant le clear
