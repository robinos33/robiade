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
