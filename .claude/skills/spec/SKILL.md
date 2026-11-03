---
name: spec
description: Tient à jour la spécification de Robiade, rangée dans specs/ avec un fichier par feature. Ce skill lit le diff d'un ticket et l'issue GitLab, trouve la ou les features touchées, met à jour leur fichier ou en crée un, et commite chaque feature dans un commit docs(spec) séparé du code. À invoquer quand un ticket est fini et sur le point d'être poussé (« c'est bon, on pousse », « prépare la MR »), quand une décision vient d'être prise en conversation (« on part là-dessus », « tranché »), ou quand on rédige un ticket, pour qu'il cite les fichiers de specs/ concernés. Mode contrôle à la demande (« la spec de la console est-elle à jour ? ») : comparer une feature au code et lister les écarts, sans rien modifier. Ne pas invoquer pour un refactor, du style ou un correctif qui rétablit un comportement déjà spécifié.
---

# Spec par feature

La spécification vit dans `specs/`. Chaque fichier décrit **une feature**, c'est-à-dire ce
qu'on peut nommer dans l'app et qu'un ticket modifie. L'index, le périmètre, le principe
directeur, les jalons et les risques sont dans `specs/README.md`. Le fichier décrit l'état actuel,
et `git log` en garde l'historique.

## Plan commun d'un fichier

```markdown
# <Nom de la feature>

**En une phrase** : …

## Comportement      ce que fait la feature, au présent
## Règles            invariants et sécurité propres à la feature
## Décisions         « Décidé le AAAA-MM-JJ : … (issue #N) », avec le risque résiduel
## Écarté            ce qu'on n'a pas fait, et pourquoi
## Tickets           - #N — titre de l'issue
```

Une section vide est omise. Un domaine devient un dossier dès qu'il a deux features. Au-delà
d'environ 150 lignes, le fichier couvre probablement deux features : le signaler.

## Mode mise à jour

### 1. Lire le ticket

La plage est donnée en argument (`<début>..<fin>`). Sinon, c'est la branche courante, après un
`git fetch -q origin main` :

```sh
PLAGE=${PLAGE:-origin/main..HEAD}
git log --format='%h %s' --invert-grep --grep='^docs(spec)' "$PLAGE"   # commits du ticket, sans la spec
git log -p --invert-grep --grep='^docs(spec)' "$PLAGE"                 # leur diff
```

Ne jamais faire `git diff origin/main..HEAD` : dès que `main` a avancé, ce diff montre à l'envers
les commits des autres. Si un diff global est nécessaire, utiliser `git diff origin/main...HEAD`
(trois points).

Pour trouver l'issue, chercher dans cet ordre : un argument, la branche (`ticket-<iid>-…`), puis
un `Closes #N` ou `Refs #N` dans les commits. La lire avec `glab issue view <iid> --comments`. Si
aucune issue n'est trouvée, travailler sur le diff seul et le dire.

### 2. Trier ce qui compte

Ne retenir que ce qui change :
- un **comportement observable** (ce que l'utilisateur voit, ce qu'une commande renvoie) ;
- une **règle de sécurité** ;
- un **choix technique** (dépendance, format, mécanisme) ;
- l'**état d'un jalon**.

Le reste est ignoré : refactor, renommage, style, tests, correctif qui rétablit un comportement
déjà écrit. **Si rien ne reste, conclure « rien à mettre à jour » sans modifier de fichier**, en
une ligne qui dit pourquoi.

### 3. Trouver la feature

Partir du diff (module, vue, commande) et de l'issue. Les correspondances ci-dessous sont des
indices, pas une vérité : lire le fichier de spec avant de décider.

| Code | Feature |
|---|---|
| `core::agents`, `adapters::{claude_code, hooks, pty, shell_env, sqlite_agents}`, vue `Agents` | `agents/lancement.md`, `agents/status.md`, `agents/statusline.md`, `agents/notifications.md` |
| `core::history`, `core::secrets`, `adapters::sqlite_search`, vue `History` | `agents/historique.md` |
| `core::{forge, merge_actions}`, `adapters::gitlab`, vue `MergeRequests` | `forge/merge-requests.md` |
| `core::{issue_actions, issue_start}` | `forge/issues.md` |
| `core::{tasks, task_start}`, `adapters::{sqlite_tasks, task_backup}`, vue `Tasks`, `robiade task` | `taches.md` |
| vue `Inbox` | `inbox.md` |
| `core::{stats, pricing}`, vue `Stats` | `stats/page-globale.md`, `stats/session-en-cours.md` |
| `core::contributions` | `stats/activite-gitlab.md` |
| `core::vcs` (worktrees), `adapters::{git, workspace}` | `environnements/worktrees.md` |
| `core::containers`, `adapters::docker` | `environnements/docker.md` |
| `core::databases`, `adapters::{vault, keychain, sqlite_connections}` | `db/connexions.md` |
| `core::db_browse`, `DbExplorer`, `DbGrid` | `db/explorateur-grille.md` |
| `core::db_console`, `adapters::{engines, sqlite_console_history}`, `DbConsole` | `db/console.md` |
| `adapters::postgres` · `adapters::mariadb` | `db/postgres.md` · `db/mariadb.md` |
| `core::code`, vue `Code` | `code.md` |
| `core::updates`, `adapters::self_update`, `scripts/install.sh` | `socle/mise-a-jour.md` |
| `core::links`, capabilities Tauri, CSP, `adapters::sqlite_audit` | `socle/securite.md` |
| thème, écran de démarrage, raccourcis, structure de l'écran | `socle/ux.md` |
| nouveau crate, dépendance, découpage du code | `socle/architecture.md` |

**Si aucune feature ne correspond, créer le fichier** selon le plan commun, à l'endroit qui suit
le rangement par domaine, et l'ajouter à l'index de `specs/README.md`. Si le ticket touche une
règle transverse de `socle/securite.md` qui ne concerne en fait qu'une feature, la déplacer dans
le fichier de cette feature.

### 4. Écrire

- Mettre à jour « Comportement » et « Règles » pour qu'ils décrivent l'état après le ticket. Une
  règle remplacée est réécrite, pas empilée : le texte ne dit jamais « remplace ».
- **Ne toucher qu'à ce que la plage change.** Un passage que le ticket ne modifie pas reste tel
  quel, même s'il semble faux : l'écart se signale dans la réponse, il ne se corrige pas.
- **Rester au niveau de la spec** : ce qu'on observe et les règles, pas l'implémentation (pas
  d'algorithme de score, pas de liste exhaustive de langages ou de raccourcis, pas de nom de
  fonction quand le comportement suffit). S'aligner sur la densité des fichiers voisins : un
  ticket ajoute quelques lignes, il ne réécrit pas le fichier.
- **Une borne de sécurité dit ce qu'elle laisse passer.** Quand le code montre la limite d'une
  protection (ce qu'elle accepte, qui peut la contourner), l'écrire comme risque résiduel : c'est
  une observation, pas une décision.
- N'ajouter une entrée à « Décisions » que s'il y a eu un **arbitrage** (une option retenue
  contre une autre, un risque accepté). Elle est datée du jour de la décision, cite l'issue et
  donne le risque résiduel s'il y en a un. Une option rejetée va dans « Écarté », avec sa raison.
- **Un arbitrage et sa raison viennent de l'issue, d'un commit ou de la conversation, jamais
  d'une déduction.** Si le code montre un choix mais que rien ne dit pourquoi, décrire le choix
  dans « Comportement » et écrire la raison **[À trancher]**, sans la deviner.
- Ajouter `- #N — <titre de l'issue>` à « Tickets » s'il n'y est pas déjà.
- Mettre à jour `specs/README.md` si un jalon change d'état, ou si le périmètre bouge.
- Garder le ton et la forme du fichier : phrases courtes, au présent, « je » pour Robin.

### 5. Commiter

Faire un commit `docs(spec): <ce qui change, du point de vue de la feature>` **par feature
touchée**, séparé des commits de code, avec `Refs #N` dans le corps. Stager fichier par fichier
(`git add specs/<fichier>`). L'index va avec la feature qui le modifie. Ne pas pousser.

## Règles

- **Ne jamais inventer une décision que Robin n'a pas prise.** Une question ouverte s'écrit
  **[À trancher]** à sa place dans la feature, et se signale dans la réponse.
- **Le texte dit ce que le code fait**, pas ce qu'il devrait faire. Si le code et la spec
  divergent sans décision qui l'explique, le signaler plutôt que d'aligner l'un sur l'autre en
  silence.
- **Une règle s'écrit à un seul endroit.** Si deux features se recouvrent, écrire la règle dans
  la principale et mettre un lien relatif depuis l'autre (`[connexions](../db/connexions.md)`).
  Signaler le recouvrement.
- **Pas de référence `fichier:ligne`**, qui se périme : on cite le module, le type ou la commande.
- Pas de numéros de section : les renvois entre features sont des liens relatifs.

## Mode contrôle

Sur demande, pour une feature (« la spec de la console est-elle à jour ? ») :
1. Lire le fichier de spec, puis le code des modules correspondants.
2. Lister les écarts sous la forme *spec dit / code fait / module*, ainsi que les comportements
   du code que la spec ne décrit pas.
3. **Ne rien modifier.** Proposer ensuite la mise à jour ou le ticket correctif, à Robin de
   choisir.

## Réponse attendue

Finir par un bilan court : les features mises à jour ou créées et leurs commits, les points
[À trancher] et les divergences entre code et spec. Ou bien « rien à mettre à jour », avec la
raison.
