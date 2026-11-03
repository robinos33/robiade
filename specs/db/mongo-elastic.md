# Mongo et Elastic

**En une phrase** : sans lecture seule de session, la protection de Mongo et d'Elastic repose sur
des identifiants en lecture seule côté serveur.

## Règles

Conséquence : pour Mongo et Elastic, la protection côté client est faible. **Une connexion
marquée lecture seule exige des identifiants en lecture seule**, l'ADE ne compense pas un compte
trop privilégié. Il l'affiche en revanche quand il le détecte (rôles Mongo, privilèges de la clé
Elastic via `_security/_authenticate`).

## Décisions

Décidé le 2026-10-28, avec Vault (voir les [connexions](connexions.md)) : pour Mongo et Elastic, qui n'ont pas de
lecture seule de session, la console de prod sera limitée à une liste blanche de commandes de
lecture.
