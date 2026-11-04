# Status d'une session

**En une phrase** : chaque session a un état, piloté par les hooks, et sa ligne porte les
indicateurs qui disent où elle en est.

## Comportement

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

**Barre au-dessus du terminal** — elle reprend le nom de l'onglet, l'état, le mode, le chemin, la
branche et la MR. Quand la place manque, le chemin cède d'abord, puis la branche et le titre de la
MR, le nom en dernier ; les actions jamais. Sous 720 px de barre, les libellés s'effacent et
restent au survol : les actions passent en icône seule, l'état en pastille seule, un mode
permissif en triangle seul. Un mode non permissif disparaît, comme sur la ligne de la liste.

## Décisions

- Décidé le 2026-11-04 : sous 720 px de barre, un mode non permissif (`Demande tout`,
  `Éditions acceptées`, `Plan`) disparaît au lieu de garder une pastille sans libellé ; la place
  revient à la branche et à la MR (issue #24). Risque résiduel : barre étroite, ces modes ne se
  lisent plus nulle part, le panneau Session ne les affiche pas.

## Écarté

- Une pastille neutre en icône seule pour un mode non permissif, qui garderait le survol : une
  icône qui ne dit rien au premier coup d'œil, ou une icône par mode, c'est trop pour ce badge.
- Mesurer la barre en JS pour basculer la variante compacte des badges : la container query fait
  le même travail en CSS.

## Tickets

- #24 — Agents — barre du terminal : badges d'état et de mode compacts quand la barre est étroite
