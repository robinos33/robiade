# Raccourcis du terminal

**En une phrase** : une barre sous le terminal d'un agent colle au curseur de son prompt une
commande ou un prompt réglé dans Réglages, sans l'envoyer.

## Comportement

- Un raccourci, c'est un **libellé** et un **texte** : une commande slash (`/simplify`) ou un
  prompt libre. La liste se règle dans la section Raccourcis des [Réglages](../socle/reglages.md) :
  ajouter, modifier, réordonner, supprimer. Elle vaut pour tous les projets et la barre suit dès
  l'enregistrement. Un profil qui n'a jamais réglé sa liste a `/simplify` et `/code-review` ; une
  liste vidée reste vide, et la barre disparaît.
- Un clic **colle le texte au curseur du prompt, sans Entrée** : j'envoie moi-même. Une commande
  slash seule est suivie d'une espace, pour taper ses arguments à la suite. Après un prompt à
  moitié tapé, le texte s'y ajoute. Le terminal reprend le focus.
- Les raccourcis sont dans la bande de la [frise du ticket](status.md), à droite de celle-ci, et
  prennent la place qu'elle laisse. Ceux qui n'y tiennent pas sont masqués ; le bouton « … » les
  donne tous.

## Règles

- **Actifs seulement quand l'agent attend un prompt** (`AwaitingInput`, `Idle`). Ailleurs, grisés,
  avec la raison au survol, et un clic ne colle rien. Pendant une demande de permission, le menu de
  Claude Code lit les touches : un collage pourrait y répondre. Pendant un tour (`Running`), un menu
  (`AskUserQuestion`) peut être ouvert sans qu'aucun hook ne le dise.
- Le texte part vers un PTY : aucun caractère de contrôle n'y entre, hormis la tabulation et le
  saut de ligne, refusés dès l'enregistrement et revérifiés au collage. Un texte sur plusieurs
  lignes n'est collé qu'en bracketed paste, où un saut de ligne ne vaut pas Entrée.
- Libellé de 24 caractères au plus, sur une ligne ; texte de 4 000 caractères au plus ; 20
  raccourcis au plus.

## Décisions

- Décidé le 2026-11-09 : **coller sans envoyer** (issue #52), plutôt qu'un envoi direct : je vois
  ce qui part, et l'envoi ne dépend pas de ce que l'agent affiche. Un raccourci ne prépare donc
  pas une commande pendant que l'agent travaille. **Risque résiduel** : un prompt `Idle` ou
  `AwaitingInput` où j'ai moi-même ouvert un menu (`/model`, `/resume`) reçoit le collage dans ce
  menu.

## Écarté

- Sortir le prompt de Claude Code dans un champ séparé : deux prompts, et le champ séparé
  perdrait l'autocomplétion `/` et `@`, l'historique, ⇧⇥, Échap et le collage d'images.
- Découvrir les raccourcis tout seul (skills du projet, plugins, commandes les plus utilisées), ou
  en avoir par projet : une liste fixe d'abord, à revoir à l'usage.
- Des raccourcis selon l'étape de la frise, et des raccourcis clavier pour les déclencher.

## Tickets

- #52 — Agents — barre de raccourcis sous le terminal : coller une commande ou un prompt sans l'envoyer
