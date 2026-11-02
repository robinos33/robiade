#!/bin/sh
# Construit Robiade et remplace /Applications/Robiade.app.
# L'application en cours d'exécution n'est pas touchée : la nouvelle version
# est prise au prochain lancement.
#
# La build embarque le commit dont elle est issue et le dépôt qui le contient :
# au lancement, l'app s'y compare à `origin/main` pour se mettre à jour.
set -eu
cd "$(dirname "$0")/.."

ROBIADE_COMMIT=$(git rev-parse HEAD)
# Le `.git` commun : le même depuis la copie de travail et depuis chacun de ses worktrees.
ROBIADE_REPO=$(git rev-parse --path-format=absolute --git-common-dir)
ROBIADE_DIRTY=
[ -z "$(git status --porcelain)" ] || ROBIADE_DIRTY=1
export ROBIADE_COMMIT ROBIADE_REPO ROBIADE_DIRTY

npm install --silent
npm --prefix ui install --silent
npm run build -- --bundles app

app=target/release/bundle/macos/Robiade.app
[ -d "$app" ] || { echo "bundle introuvable : $app" >&2; exit 1; }

rm -rf /Applications/Robiade.app.new
cp -R "$app" /Applications/Robiade.app.new
rm -rf /Applications/Robiade.app
mv /Applications/Robiade.app.new /Applications/Robiade.app
echo "Installé : /Applications/Robiade.app ($ROBIADE_COMMIT${ROBIADE_DIRTY:+, modifications non commitées})"
