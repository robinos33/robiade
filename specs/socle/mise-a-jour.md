# Mise à jour depuis `origin/main`

**En une phrase** : au lancement, l'app installée se reconstruit depuis `origin/main` si elle est
en retard, et propose de relancer.

## Décisions

Décidé le 2026-11-02 : **l'app se met à jour depuis `origin/main` au lancement** (issue #15).
`scripts/install.sh` embarque le commit construit, le `.git` commun et un marqueur de
modifications non commitées ; le commit s'affiche au survol du nom de l'app. Au lancement, un
`fetch` d'`origin/main` (abandonné après 30 s, la ref locale sert alors), puis
`rev-list --count <installé>..origin/main` : à zéro, rien n'est construit — une app installée
depuis une branche en avance sur `main` compte comme à jour. En retard, `origin/main` est extrait
en tête détachée dans un worktree réservé du cache (`~/Library/Caches/dev.robin.robiade/update`,
hors Time Machine, gardé pour que `target/` et `node_modules` restent chauds), et c'est **son**
`install.sh` qui construit et installe. La barre latérale dit « Mise à jour en cours », puis
« Relancer pour mettre à jour (N commits) » ; un échec montre la fin de la sortie et le chemin du
journal complet, et l'app réessaiera au lancement suivant. La relance passe par la confirmation de
fermeture **avant** d'être demandée : Tauri ignore `prevent_exit` sur une relance. Pas de
vérification périodique : au lancement seulement. Un `tauri dev`, ou une build qui ne vient pas
d'`install.sh`, ne se met jamais à jour ; la première installation qui embarque le commit se fait
à la main.

## Tickets

- #15 — App : mise à jour automatique depuis main au démarrage
