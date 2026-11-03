#!/bin/sh
# Régénère les captures du README depuis l'interface réelle et le faux backend de demo/mock.ts.
# Prérequis : le serveur Vite tourne (npm --prefix ui run dev, ou npm run dev à la racine).
set -eu
cd "$(dirname "$0")/../.."
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
PROFILE=$(mktemp -d)
trap 'rm -rf "$PROFILE"' EXIT
mkdir -p docs/screens

shot() {
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --user-data-dir="$PROFILE" \
    --window-size=1680,1050 --force-device-scale-factor=2 --virtual-time-budget=4000 \
    --screenshot="docs/screens/$1.png" "http://localhost:5173/demo/?$2" 2>/dev/null
}

shot agents 'view=agents'
shot resume 'view=agents&dialog=resume'
shot mr 'view=mr'
shot stats 'view=stats'
shot code 'view=code&project=boutique&file=src/Cart/PromoCodePolicy.php'
