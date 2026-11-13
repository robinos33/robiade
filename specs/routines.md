# Routines

**En une phrase** : mes tâches planifiées, des LaunchAgents que launchd lance tout seul et que
Robiade montre, lance à la demande, active, désactive, crée et modifie.

## Comportement

- **Une routine est un LaunchAgent** de `~/Library/LaunchAgents` dont le label commence par le
  préfixe des routines, réglé dans [Réglages](socle/reglages.md). Le plist est la source de
  vérité, et launchd son état : Robiade n'en garde rien.
- **Vue Routines** (⌘9), globale, sans rattachement à un projet. Pour chaque routine : son nom (le
  label sans le préfixe), son planning en clair (« lun–ven 9h00 »), son prochain passage, son
  dernier passage (code de sortie ou signal) et son état : active, en cours avec son PID,
  désactivée, ou non chargée. Le planning se lit comme launchd le comprend, une clé absente
  valant n'importe quelle valeur, et le prochain passage se calcule sur l'heure locale.
- **L'heure du dernier passage est approximative** : launchd ne la garde pas. C'est la dernière
  écriture d'un des journaux de la routine, et l'écran le dit.
- **Détail** : la commande, le dossier de travail, le plist, la fin des journaux stdout et
  stderr (32 Ko chacun au plus), et deux boutons qui montrent dans le Finder le plist ou le
  script lancé (le premier argument qui est un fichier, sinon le programme). Rien n'est ouvert
  ni exécuté. **[À trancher]** : l'issue prévoyait d'ouvrir le script dans l'onglet Code, qui ne
  lit que les fichiers d'un projet, alors qu'un script de routine vit souvent ailleurs.
- **Actions**, chacune confirmée sur la routine relue juste avant, et inscrite au journal d'audit :
  - « Lancer » : `launchctl kickstart`. Les effets de la commande partent pour de vrai ;
  - « Désactiver » : `disable` puis `bootout`. Elle ne part plus et ne revient pas à la session
    suivante. Le plist reste ;
  - « Activer » : `enable` puis `bootstrap` ;
  - « Supprimer » : `bootout`, puis suppression du plist. Les journaux restent.
- **Créer une routine** : un nom, un programme en chemin absolu et ses arguments, un champ
  chacun, un dossier de travail optionnel, des jours et des heures. Le label est le préfixe
  suivi du nom. Une routine neuve journalise sous `~/Library/Logs/Robiade/routines/<nom>.out.log`
  et `.err.log`, et reçoit le `PATH` de mon shell interactif : sans lui, launchd n'a que
  `/usr/bin:/bin:/usr/sbin:/sbin`. **[À trancher]** : le formulaire ne propose pas d'autres
  journaux, alors que l'issue les disait seulement « par défaut ». Robiade montre le plist exact avant de l'écrire et ne
  l'enregistre qu'après confirmation. Il le charge ensuite, et le retire si launchd le refuse.
- **Routine de skill** : le formulaire propose deux types, « Commande » et « Skill Claude Code ».
  Le type Skill demande un dossier, obligatoire (mes projets sont proposés), un skill, ses
  arguments sur une ligne, un modèle et un effort (ceux de Claude Code, ou son réglage par défaut),
  un nombre maximal de tours (30 par défaut) et les outils autorisés, une règle de permission par
  ligne. Les skills proposés sont ceux que `claude` trouve depuis ce dossier, hors commandes
  intégrées : les miens, ceux du projet, des plugins et de claude.ai. Le plist reste une routine
  ordinaire, dont la commande est `claude -p "/<skill> <arguments>"` suivie de `--max-turns`, du
  modèle, de l'effort et de `--allowedTools`. Le détail montre le skill lancé. Au-delà du nombre de
  tours, `claude` sort en échec et la routine remonte dans l'Inbox. La session du passage est
  gardée : elle compte dans les stats, mais n'apparaît pas dans l'historique, comme toute session
  lancée en `-p`. Le résultat se lit dans le journal stdout.
- **Modifier une routine** : le même formulaire, sans changer le nom. Les journaux et le `PATH` du
  plist sont gardés. Robiade décharge la routine, réécrit le plist, puis la recharge, sauf si elle
  est désactivée. Si launchd refuse le nouveau plist, l'ancien revient et la routine est
  rechargée telle qu'elle tournait. launchd remet alors à zéro le compte des passages et le
  dernier code de sortie.
- **Une routine en échec à son dernier passage remonte dans l'[Inbox](inbox.md)** et porte une
  pastille sur la vue. Robiade relit launchd toutes les 5 minutes, et toutes les 10 secondes tant
  que la vue est ouverte.

## Règles

- **Robiade ne lit, n'écrit et ne pilote que les LaunchAgents du préfixe.** Un label venu de
  l'interface est refusé s'il n'est pas du préfixe. Les autres services de
  `~/Library/LaunchAgents` ne sont ni lus ni modifiés, et `LaunchDaemons` n'est jamais touché. Le
  préfixe a au moins deux segments et ne peut pas commencer par `com.apple.`.
- **Un label ne sort pas du dossier** : il ne contient que des lettres, des chiffres, `.`, `-` et
  `_`, et ne commence pas par un point. Le fichier doit s'appeler comme le label du plist, sinon
  la routine est montrée comme illisible. Un lien symbolique n'est pas une routine.
- **Aucun shell** : `launchctl` et `plutil` sont lancés par leur chemin absolu, avec leurs
  arguments en liste. Le plist écrit n'a que `ProgramArguments` : le programme est un chemin
  absolu vers un fichier exécutable, vérifié avant l'écriture. Un pipe ou une redirection passe
  par un `/bin/sh -c` explicite, visible dans l'aperçu.
- **Seules les clés que Robiade écrit sont modifiables** : `Label`, `ProgramArguments`,
  `WorkingDirectory`, `StandardOutPath`, `StandardErrorPath`, `EnvironmentVariables` réduit au
  seul `PATH`, et un `StartCalendarInterval` qui croise des jours et des heures. Toute autre clé
  (`Program`, `StartInterval`, `RunAtLoad`, `KeepAlive`…), ou une clé connue mais mal formée,
  laisse la routine en lecture seule, avec la raison. Elle reste lançable, activable et
  supprimable.
- **Une routine de skill ne contourne pas les permissions.** Le formulaire ne propose ni
  `--dangerously-skip-permissions` ni `--permission-mode`. Une routine n'est relue comme routine de
  skill que si sa commande a exactement la forme que Robiade écrit ; un flag de plus, et c'est une
  routine « Commande ». Sans règle, le skill lit et fait ce que mes réglages de Claude Code
  autorisent déjà ; le reste est refusé sans question.
- **`claude` ne part que dans un dossier que Claude Code a approuvé.** `claude -p` ne pose pas la
  question de confiance et applique les réglages du projet, dont certains lancent des commandes.
  La lecture des skills d'un dossier et l'enregistrement d'une routine de skill sont donc refusés
  si ni le dossier, ni un dossier qui le contient, n'est approuvé dans `~/.claude.json`. Le chemin
  est résolu avant (un `..` ou un lien ne passe pas), et la racine ou le dossier personnel
  approuvés n'approuvent qu'eux-mêmes. La lecture des skills lance `claude` sans session ni hook.
- **Une routine en cours ne se décharge pas** : désactiver, modifier ou supprimer attend la fin,
  car `bootout` tuerait le programme en plein travail. La lancer à nouveau est refusé.
- **Une écriture ne laisse pas de plist à moitié écrit.** Le plist est écrit à côté de sa place
  et validé par `plutil -lint`, puis mis en place. À la création, il n'écrase jamais un fichier
  existant.
- **Risque résiduel** : un outil refusé à un skill ne fait pas échouer le passage. `claude` sort
  avec 0, et la routine ne remonte pas dans l'Inbox : seul son journal le dit. Approuver un dossier
  dans Claude Code approuve aussi, pour Robiade, tous ses sous-dossiers.
- **Risque résiduel** : la création appelle `launchctl enable` avant de charger la routine. Une
  routine supprimée puis recréée sous le même nom repart donc active, même si elle avait été
  désactivée. launchd garde aussi une trace de chaque label désactivé ou réactivé, que rien ne
  permet d'effacer.

## Décisions

Décidé le 2026-11-10 : **launchd reste le moteur des routines**, Robiade n'en planifie aucune
(issue #62). launchd rattrape au réveil un créneau manqué pendant la veille, et une routine survit
à une relance de Robiade (#15) ou à un plantage. Robiade est ouvert presque en permanence chez
moi : ce n'est pas l'argument.

Décidé le 2026-11-13 : **une routine peut lancer un skill Claude Code dans un dossier** (issue
#69). Le type « prompt Claude » écarté par #62 est rouvert, la condition posée étant remplie (une
deuxième routine qui appelle `claude -p`), mais restreint aux skills. Il n'ajoute aucune clé au
plist : c'est une forme de commande, que launchd lance comme les autres.

Décidé le 2026-11-10 : **le préfixe d'un profil neuf est `dev.robin.robiade.routine.`**, celui de
l'identifiant de l'application (issue #62). Je règle le mien pour retrouver les routines que
j'avais créées avant.

## Écarté

- Un planificateur interne à Robiade : ce serait un deuxième moteur à sécuriser, moins fiable que
  launchd face à la veille et aux relances.
- Les LaunchDaemons, et toute routine qui tourne en root.
- Un prompt libre au lieu d'un skill : un skill porte déjà le prompt, se teste à la main avant
  d'être planifié et garde la routine lisible. Un prompt libre reste possible avec le type
  « Commande ».
- Déduire les outils autorisés du contenu d'un skill.
- Juger la réussite d'un skill sur sa sortie : seul le code de sortie compte, comme pour toute
  routine.
- L'historique des passages (heure exacte, durée, code de chacun) : launchd ne le garde pas, et
  envelopper la commande rendrait la routine dépendante de Robiade.
- Éditer `StartInterval`, `WatchPaths` et les autres déclencheurs.

## Tickets

- #62 — Routines — voir, lancer et créer mes tâches planifiées launchd
- #69 — Routines — lancer un skill Claude Code dans un dossier, à heure fixe
