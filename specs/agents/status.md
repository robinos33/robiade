# Status d'une session

**En une phrase** : chaque session a un état, piloté par les hooks, et sa ligne porte les
indicateurs qui disent où elle en est ; sous le terminal, une frise dit où en est son ticket.

## Comportement

**Status d'une session** — machine à états, pilotée par les hooks :

| État | Déclencheur | Rendu |
|---|---|---|
| `Starting` | lancement du process, jusqu'au premier hook (`SessionStart` le fait passer en `Idle`) | arc qui tourne, distinct d'`Running` même sans libellé : un agent qui y reste ne reçoit pas ses hooks |
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

**Frise du ticket**, une ligne fixée sous le terminal, aussi en pleine fenêtre : les étapes du
ticket de l'agent ouvert, et à droite ce qui retient sa MR. Chaque étape vient d'un fait observé,
relu à chaque lecture ; rien n'est stocké, et une MR repassée en brouillon ramène la frise en arrière.
- **Ticket #N** : la branche extraite dans le dossier de l'agent est `ticket-<N>-…`, celle d'un
  agent lancé depuis une issue ou depuis une tâche liée à une issue ; sinon, la branche source de
  sa MR. Sinon, pas d'étape Ticket.
- **Exploration**, puis **Code** dès que l'agent a écrit un fichier de son dossier de travail
  (Write, Edit). Le scratchpad, sa mémoire ou un autre dépôt ne comptent pas ; le compteur
  « Fichiers modifiés » du panneau Session, lui, compte toujours tout.
- **Brouillon**, **Revue**, **Mergée** : l'état de la MR de l'agent (celle de sa branche, sinon
  celle de son dernier `pr-link`), lue sur la forge comme le dit la
  [lecture des MR](../forge/merge-requests.md). Un wagon mergé affiche « Mergée dans
  `<cible>` ». Une MR fermée sans merge prend la place de Revue, barrée d'une croix rouge.
- **Ce qui retient la MR**, le plus urgent seulement, à partir de Brouillon : conflits, pipeline en
  échec, annulé ou sur un job manuel, discussions non résolues, CI en cours, puis, en Revue
  seulement, approbations manquantes, un autre refus de la forge (son statut de merge), ou
  « prête à merger ». Marqué **à faire**, ou **en attente** quand il attend la CI, les relecteurs
  ou la forge. Des approbations illisibles s'affichent « inconnues », jamais « prête ».
- Nœuds passés pleins, nœud courant cerclé, nœuds à venir creux ; un échec barre le nœud d'une
  croix. Sous 720 px, les autres étapes gardent leur pastille et leur libellé passe au survol.
- MR illisible (jeton, réseau) : « MR illisible », et l'étape s'arrête à ce que l'agent a fait.
- La barre du terminal prend la MR de la frise, avec son état et son titre sur les deux forges.

## Décisions

- Décidé le 2026-11-04 : sous 720 px de barre, un mode non permissif (`Demande tout`,
  `Éditions acceptées`, `Plan`) disparaît au lieu de garder une pastille sans libellé ; la place
  revient à la branche et à la MR (issue #24). Risque résiduel : barre étroite, ces modes ne se
  lisent plus nulle part, le panneau Session ne les affiche pas.

- Décidé le 2026-11-09 : **une frise du ticket fixée sous le terminal**, plutôt qu'en bas du
  panneau Session (issue #51) : la largeur du terminal tient les six étapes sur une ligne, la frise
  ne défile pas avec le panneau, et elle reste visible en pleine fenêtre, quand la liste est
  cachée. Le terminal y perd une ligne. Chaque étape est déduite d'un fait observé, recalculée à
  chaque lecture, sans état stocké. **Risques résiduels** : un agent repris sur une branche qui
  porte déjà du code, sans MR, s'affiche en Exploration tant qu'il n'écrit rien (les écritures ne
  survivent pas à une relance de Robiade) ; une modification faite par Bash (`sed -i`, `git apply`)
  ne fait pas passer en Code.

## Écarté

- Une pastille neutre en icône seule pour un mode non permissif, qui garderait le survol : une
  icône qui ne dit rien au premier coup d'œil, ou une icône par mode, c'est trop pour ce badge.
- Mesurer la barre en JS pour basculer la variante compacte des badges : la container query fait
  le même travail en CSS.
- Des étapes « Analyse » ou « Tests » devinées d'après les outils appelés : la frise ne promet que
  ce qu'elle observe. Des étapes « commitée » ou « poussée » : l'existence de la MR suffit. Une
  étape « ticket fermé » : `Closes #N` s'en charge au merge.
- La frise réduite sur chaque ligne de la liste des agents : les lignes sont déjà chargées, à
  rouvrir après usage.

## Tickets

- #24 — Agents — barre du terminal : badges d'état et de mode compacts quand la barre est étroite
- #51 — Agents — frise du cycle de vie du ticket, fixée sous le terminal
- #56 — Agents — la frise ignore la PR d'un agent qui travaille en worktree depuis main
- #67 — Agents — « Démarrage » a la même pastille que « En cours » : indiscernables en compact
