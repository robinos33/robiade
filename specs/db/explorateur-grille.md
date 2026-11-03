# Explorateur et grille

**En une phrase** : parcourir connexions, bases et tables, et lire une table dans une grille
paginée, en lecture seule.

## Comportement

- **Explorateur** : connexions → bases → schémas → tables / collections / index.
- **Grille** : ouvrir une table affiche ses lignes dans une grille virtualisée, triable et
  filtrable. Pagination côté serveur (keyset quand une clé le permet, sinon `LIMIT/OFFSET`) :
  on ne charge jamais une table entière. Valeurs JSON et longues dans un panneau de détail.
- Mongo et Elastic n'ont pas de tables : la grille aplatit les documents sur leurs champs de
  premier niveau, et le reste s'ouvre en arbre JSON.

## Écarté

- **Pas d'édition de cellule en v1.** Une édition en grille est une écriture déguisée, avec un
  ciblage (clé primaire, documents sans clé stable) facile à rater. Les écritures passent par
  la console, sur une connexion en mode écriture, avec confirmation.
