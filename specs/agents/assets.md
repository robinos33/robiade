# Assets d'un agent

**En une phrase** : les fichiers qu'un agent écrit pour que je les regarde (captures, pages,
notes, et tout ce qu'il range dans son scratchpad) se listent sous son terminal et s'ouvrent
dans le visualiseur de Robiade.

## Comportement

- Un fichier devient un asset quand un hook rapporte que l'agent l'a écrit par Write ou Edit, ou
  qu'il a lu une image. Le dernier écrit passe en tête de la liste ; un fichier réécrit remonte
  sans être dupliqué. La liste vit avec l'agent et n'est pas gardée d'une exécution de Robiade à
  l'autre.
- Hors du scratchpad, seuls les images, le HTML, le Markdown et le texte simple (`.txt`, `.csv`)
  sont des assets : le code du projet n'en est pas un.
- **Dans le scratchpad de l'agent, tout fichier écrit est un asset, quelle que soit son
  extension.** Ce qui n'est ni image, ni HTML, ni Markdown s'affiche en texte, coloré d'après
  l'extension comme dans l'onglet Code. L'asset est gardé sous son chemin résolu : `/tmp/…` et
  `/private/tmp/…` n'en font qu'un.
- Le scratchpad d'une session est `<racine>/<dossier de lancement>/<session>/scratchpad`, où la
  racine est `$CLAUDE_CODE_TMPDIR/claude-<uid>` résolu (`/tmp` à défaut), comme Claude Code
  2.1.318 le construit. Un agent connaît toutes ses sessions : la première, celle qu'il reprend
  le cas échéant (une reprise garde l'identifiant), et une nouvelle à chaque `/clear`, que les
  hooks rapportent. Chacune a son scratchpad.
- Un asset s'ouvre avec les règles de l'onglet Code ([Code](../code.md)) : au-delà de 1 Mo de
  texte ou de 20 Mo d'image, ou avec un octet nul en tête, il reste dans la liste et le
  visualiseur dit pourquoi il ne l'affiche pas. Les jetons connus sont masqués.

## Règles

- **Seul le scratchpad de l'agent est concerné** : ni le reste de `/tmp`, ni celui d'une autre
  session. Le chemin est résolu côté Rust (`..`, liens) avant d'être comparé.
- Un fichier de secrets (`.env`, clé privée…), même écrit dans le scratchpad, reste dans la liste
  mais n'est jamais affiché, même à ma demande ; un lien nommé comme tel, ou qui mène à un tel
  fichier, non plus.
- Un identifiant de session venu d'un hook n'est gardé que s'il a la forme d'un UUID.

## Décisions

Décidé le 2026-11-05 : **le scratchpad se reconnaît à la session, pas au dossier de lancement**
(issue #40). Claude Code réduit ce dossier à un nom qu'il hache au-delà de 200 caractères ;
plutôt que de recopier cet algorithme, n'importe quel nom est accepté à cette place, et
l'identifiant de session, un UUID propre à l'agent, suffit à exclure les autres agents.
**Risque résiduel** : l'emplacement du scratchpad est un format interne de Claude Code, non
documenté. S'il change, les fichiers du scratchpad cessent d'être reconnus : ils redeviennent
des fichiers ordinaires, sans rien ouvrir de plus.

## Écarté

- Un fichier que l'agent crée dans son scratchpad par une commande (`cat > … <<EOF`) n'entre pas
  dans la liste : les hooks ne voient que les écritures par Write ou Edit. Il s'ouvre quand même
  par ⌘+clic sur son chemin cité.
- Lister le contenu du scratchpad sans écriture observée, et nettoyer les scratchpads : hors du
  périmètre de l'issue #40.

## Tickets

- #40 — Agents — un fichier écrit dans le scratchpad est un asset, quelle que soit son extension
