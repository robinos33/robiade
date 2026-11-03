# Page de stats globale

**En une phrase** : les stats de toutes les sessions, filtrables par projet et par période, dont
le temps où un agent m'a attendu.

## Comportement

Calculées à partir des transcripts et des événements de hooks, stockées en agrégats SQLite
reconstructibles. Aucune nouvelle source.

**Page globale**, filtrable par projet et par période :
- sessions par jour, durée médiane, taux de `Failed` ;
- tokens et coût estimé par projet et par modèle ;
- outils les plus utilisés ; demandes de permission les plus fréquentes (candidates à une règle
  `allow`, à décider à la main) ;
- **temps où un agent m'a attendu** (`AwaitingPermission` + `AwaitingInput` non lu) : c'est la
  mesure de qui est le goulot, l'agent ou moi ;
- MR produites par des sessions, et part de celles qui ont été mergées.

## Règles

Le coût est une **estimation** : tokens × grille tarifaire versionnée dans l'ADE. Avec un
abonnement, ce n'est pas ce que je paie. L'UI met les tokens en premier et le coût en second.
