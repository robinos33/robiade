# Réglages

**En une phrase** : la page Réglages règle l'instance de forge, le Vault et les branches de tronc
de chaque projet, sans toucher au code ; rien n'y est secret.

## Comportement

- Page **Réglages** (icône en bas de la barre latérale, palette, ⌘,), enregistrée dans
  `robiade.db`. Trois sections :
  - **Forge** : fournisseur (GitLab seul), URL de l'instance, service du trousseau du jeton
    (défaut `gitlab-api-token`). Vide : pas de volet Forge (merge requests, issues, activité),
    comme pour un dépôt hors de l'instance.
  - **Vault** (optionnel) : URL, service du trousseau de secours (défaut `vault-token`). Vide : la
    source « Vault » disparaît du dialogue de connexion DB.
  - **Projets** : les branches de tronc de chaque dépôt, par ordre de préférence ; vide, la branche
    par défaut du remote. Elles décident de la base des [worktrees](../environnements/worktrees.md)
    et des wagons des [merge requests](../forge/merge-requests.md). Rattachées au dépôt principal,
    elles valent pour tous ses worktrees.
- Forge et Vault sont lus au démarrage : un changement enregistré s'applique à la relance, que la
  page propose. Les troncs s'appliquent tout de suite.
- **Profil antérieur** (une base qui existait avant les réglages) : au premier lancement, les
  réglages sont repris une seule fois là où mes outils les gardent déjà — l'hôte par défaut de
  `glab` pour la forge, `VAULT_ADDR` lu dans un shell interactif pour Vault — et chaque dépôt déjà
  connu (historique des sessions, tâches) reçoit `staging`, `main`, `master`. Un profil neuf
  démarre sans forge, sans Vault et sans troncs.

## Règles

- Aucun secret dans les réglages : un jeton se désigne par son service du trousseau.
- Une URL d'instance est `https://hôte[:port]`, sans chemin, requête ni identifiants.
- Le jeton ne part que vers l'hôte réglé. **Un nouvel hôte n'est enregistré qu'une fois
  confirmé**, premier réglage compris (un jeton peut déjà attendre dans le trousseau) : Rust
  renvoie l'hôte normalisé, la confirmation le montre avec le service du jeton, puis le renvoie.
  Retirer un réglage n'envoie rien et ne demande pas de confirmation. Même règle pour Vault, dont
  le jeton de la CLI partirait lui aussi vers le nouvel hôte.
- **Risque résiduel** : la reprise d'un profil antérieur enregistre l'hôte de `glab` sans
  confirmation. Si cet hôte n'est pas l'instance pour laquelle le jeton `gitlab-api-token` a été
  créé, le jeton part vers lui. Chez moi les deux coïncident.

## Décisions

Décidé le 2026-11-04 : **une seule instance de forge** (issue #26).

Décidé le 2026-11-04 : **les branches de tronc se règlent projet par projet**, sans liste globale
codée en dur (issue #26). Un dépôt sans réglage suit la branche par défaut de son remote.

Décidé le 2026-11-04 : **forge et Vault s'appliquent à la relance** (issue #26). Les appliquer à
chaud imposerait de reconstruire les adaptateurs et de vider les caches (états des MR,
contributions, jeton, utilisateur) pour un réglage qu'on change une fois.

Décidé le 2026-11-04 : **la reprise d'un profil antérieur lit `glab` et `VAULT_ADDR`** plutôt que
des hôtes écrits dans le code (issue #26) : le code ne nomme plus aucune entreprise. Risque
résiduel ci-dessus.

## Écarté

- Bitbucket (Robin, 2026-11-04) : Cloud et Data Center sont deux API distinctes, et les issues y
  vivent souvent dans Jira. Les fournisseurs se limitent à GitLab et GitHub (#28).
- Autres plateformes que macOS : le trousseau, `tmutil` et `open` sont propres à macOS.

## Tickets

- #26 — Socle — indépendance vis-à-vis de l'entreprise : instance GitLab, Vault et branches de tronc dans une page Réglages
