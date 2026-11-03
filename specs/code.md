# Onglet Code

**En une phrase** : parcourir et lire les fichiers du projet ou du worktree sélectionné, en
lecture seule.

## Décisions

Décidé le 2026-11-03 : **un onglet Code, visualiseur en lecture seule** (issue #7). La liste des
fichiers est celle de `git ls-files --cached --others --exclude-standard` : `.gitignore` respecté,
fichiers non suivis d'un agent inclus, sans hook ni filtre git. Elle part entière vers l'interface
(10 000 fichiers sur le plus gros dépôt du poste), qui en construit l'arbre et y cherche par nom
(`⌘P`). L'aperçu est celui des assets des agents, avec une coloration CodeMirror en lecture seule ;
les langages se chargent à la demande. Au-delà de 1 Mo de texte (20 Mo d'image), ou si le début du
fichier contient un octet nul, rien n'est lu : un message le dit. Les fichiers de secrets connus
restent dans l'arbre, marqués, et leur contenu ne se charge que sur « Afficher quand même » : le
`.env` d'un projet Symfony est commité et utile à lire. **Risque résiduel** : la racine acceptée
est tout dépôt ou worktree Git du disque, Robiade ne tenant pas de liste de projets côté Rust ; une
WebView compromise pourrait donc lire un autre dépôt que ceux de l'interface.

## Tickets

- #7 — Code : onglet de navigation et lecture des fichiers d'un projet
