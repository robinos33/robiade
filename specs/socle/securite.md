# Sécurité

**En une phrase** : les principes de sécurité transverses ; une règle propre à une seule feature
a vocation à rejoindre le fichier de cette feature.

## Règles

L'ADE concentre un jeton de forge, des accès DB et le socket Docker : c'est une cible de valeur.
Le niveau d'exigence est celui d'un outil d'administration.

### Surface d'attaque
- **Aucun port réseau en écoute.** Le canal hooks → ADE est un socket Unix dans
  `~/Library/Application Support/ade/`, permissions `0600`, et l'ADE vérifie l'UID du pair
  (`getpeereid`).
- Sorties réseau **sur liste blanche** : les hôtes configurés (l'API de la forge et Vault, voir
  les [réglages](reglages.md)) et les hôtes DB déclarés.
- Tauri : capabilities minimales commande par commande ; pas de plugin `shell`, `fs` ni `opener`
  exposé au front : une page s'ouvre dans le navigateur par une commande Rust qui n'accepte qu'une
  URL `https` de l'instance de forge réglée, sans identifiants ni autre port ; CSP stricte (`default-src 'self'`, pas d'`unsafe-inline`, pas d'`unsafe-eval`) ; aucun
  contenu distant chargé dans la WebView.

### Secrets
- Keychain uniquement (jeton de forge, mots de passe DB). Jamais dans SQLite, les logs, les messages
  d'erreur ou l'IPC vers le front.
- Type `Secret<T>` (`secrecy`) côté Rust : pas de `Debug`/`Display`, effacé en mémoire au drop.

### Données non fiables
- Tout ce qui vient des transcripts, de la forge (titres, descriptions, commentaires, logs de jobs)
  et de Docker est **non fiable** : rendu comme texte, jamais en `{@html}`. Markdown et HTML rendus
  dans une iframe `srcdoc` sandboxée (ni script, ni origine, ni navigation hors de l'app) : aucun
  nettoyage à maintenir, le contenu ne peut rien exécuter. Les liens y sont inertes ; les images
  jointes à GitLab, servies derrière authentification, ne sont pas chargées et s'affichent comme
  leur adresse.
- Un transcript peut contenir des secrets collés par erreur : la recherche plein texte est locale
  et son index SQLite vit dans le cache de l'app, exclu de Time Machine (`tmutil addexclusion`).
- Lecture des fichiers d'un projet (onglet Code) : une commande Rust, bornée à la racine du dépôt
  ou du worktree (ni `..`, ni chemin absolu, ni lien qui en sort une fois résolu), fichiers
  ordinaires seulement, taille vérifiée avant lecture. Les fichiers de secrets connus (`.env*`,
  clés privées, `auth.json`…) ne sont lus qu'à ma demande ; le reste passe par le même masquage
  de jetons que l'historique.
- Liens du terminal d'un agent (⌘+clic) : la sortie d'un agent n'est pas fiable (page, issue ou
  fichier relu). Une seule fonction d'ouverture : une page de la forge réglée vers le navigateur (tout autre lien web reste sans effet), `file://`
  vers le visualiseur de Robiade, jamais vers une autre app ; tout autre schéma est ignoré, sans
  boîte de dialogue. Un `file://` est décodé et résolu (`..`, liens) côté Rust, puis lu seulement
  s'il est un asset de l'agent, s'il est dans son scratchpad ([assets](../agents/assets.md)) ou
  s'il reste dans son dossier de travail, avec les règles de
  l'onglet Code ; un fichier de secrets n'est pas affiché, même à ma demande. Un chemin cité sans
  `file://` (`src/foo.rs:42`) suit la même lecture. Savoir s'il désigne un fichier, au survol,
  passe aussi par Rust, avec les mêmes bornes : la réponse ne renseigne pas sur le reste du disque.

### Actions
- Toute action sortante (forge, DB en écriture, suppression de worktree, arrêt de conteneur)
  passe par une confirmation qui montre **la cible exacte** (projet, MR, branche). Pas de
  confirmation générique « êtes-vous sûr ? ».
- DB : connexion en lecture seule par défaut ; l'écriture se fait avec un autre utilisateur ou
  une bascule explicite, par connexion. **Prod en lecture seule, sans bascule**, sauf dans la
  console : INSERT, UPDATE ou DELETE un à un, en deux temps (nom de la connexion tapé, puis
  validation sur le nombre de lignes) ; TLS exigé hors dev ([connexions DB](../db/connexions.md)).
- Journal d'audit local des actions sortantes (quoi, quand, cible, résultat), sans les secrets.

### Chaîne de build
- `cargo deny` (licences, advisories), `cargo audit`, `npm audit` en CI ; lockfiles commités.
- **Mise à jour au lancement depuis `origin/main`** ([décision du 2026-11-02](mise-a-jour.md)) : un commit poussé sur
  `main` est construit et installé sans relecture, au lancement suivant. C'est le dépôt perso, où
  je pousse en direct, agents compris : risque assumé. La build tourne avec le PATH de mon shell
  de login, sans hook git, dans un worktree réservé ; jamais dans ma copie de travail.
- Passes de mes skills `xss-audit` sur `ui/` à chaque jalon (la WebView est le maillon le plus
  exposé) ; `csrf-safe-method-audit` sans objet tant qu'il n'y a pas de HTTP.

## Tickets

- #7 — Code : onglet de navigation et lecture des fichiers d'un projet
- #11 — Terminal : ⌘+clic sur un lien
- #26 — Socle — indépendance vis-à-vis de l'entreprise : instance GitLab, Vault et branches de tronc dans une page Réglages
- #13 — Terminal : chemins cliquables, ⌘F, glisser-déposer, taille de police
- #15 — App : mise à jour automatique depuis main au démarrage
- #40 — Agents — un fichier écrit dans le scratchpad est un asset, quelle que soit son extension
- #28 — Forge — fournisseur GitHub (pull requests, issues, checks)
