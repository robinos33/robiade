#!/bin/sh
# Construit Robiade et remplace /Applications/Robiade.app.
# L'application en cours d'exécution n'est pas touchée : la nouvelle version
# est prise au prochain lancement.
set -eu
cd "$(dirname "$0")/.."

npm install --silent
npm --prefix ui install --silent
npm run build -- --bundles app

app=target/release/bundle/macos/Robiade.app
[ -d "$app" ] || { echo "bundle introuvable : $app" >&2; exit 1; }

rm -rf /Applications/Robiade.app.new
cp -R "$app" /Applications/Robiade.app.new
rm -rf /Applications/Robiade.app
mv /Applications/Robiade.app.new /Applications/Robiade.app
echo "Installé : /Applications/Robiade.app"
