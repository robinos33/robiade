# Modèle et effort d'un agent

**En une phrase** : les lignes Modèle et Effort du panneau Session sont des listes ; un choix
relance l'agent sur sa session avec ce modèle et cet effort, sans toucher aux réglages de Claude
Code.

## Comportement

- Modèle : « défaut », puis les alias de Claude Code (`fable`, `opus`, `sonnet`, `haiku`). Effort :
  « défaut », puis `low` à `max`. « défaut » ne passe aucun flag : le réglage de Claude Code
  s'applique. Sans choix, l'effort en service nomme le défaut (« high (défaut) ») ; le modèle en
  service est dans la barre de jauges, au-dessus du terminal.
- Un choix **relance l'agent sur sa session** par le chemin de « Redémarrer », avec `--model` et
  `--effort`. La conversation continue, avec la nouvelle valeur.
- **Le choix appartient à l'onglet** : il est gardé d'une exécution de Robiade à l'autre et repassé
  à chaque reprise de l'onglet (« Reprendre », « Redémarrer »). Une session reprise depuis
  l'historique sans onglet repart sur le défaut.
- Un effort en service différent du choix (un `/effort` tapé dans le terminal) s'affiche à côté de
  la liste.

## Règles

- **Actif seulement quand rien n'est en cours** : `AwaitingInput`, `Idle` ou `Ended`, où le choix
  reprend la session. Ailleurs, les listes sont grisées avec la raison au survol, et la commande
  refuse : une relance couperait le tour ou la demande de permission.
- Seules les valeurs des listes partent en arguments de `claude` ; toute autre valeur est refusée,
  et une valeur enregistrée sortie des listes depuis est oubliée à la relecture.
- `~/.claude/settings.json` n'est jamais écrit : `--model` et `--effort` ne valent que pour le
  process lancé (vérifié sur Claude Code 2.1.322).

## Décisions

Décidé le 2026-11-10 : **le sélecteur est dans le panneau Session**, à la place de l'affichage du
modèle et de l'effort, et pas au lancement (issue #59).

Décidé le 2026-11-10 : **relancer avec `--model` et `--effort`** plutôt que taper `/model` dans le
terminal (issue #59). Sur Claude Code 2.1.322, `/model sonnet` et `/effort high` tapés dans le
prompt deviennent le défaut global de Claude Code, iTerm compris. **Risque résiduel** : la relance
perd un prompt à moitié tapé. L'état est vérifié avant la relance, pas pendant : un prompt envoyé
entre les deux voit son tour coupé.

## Écarté

- Taper `/model X` ou `/effort X` dans le PTY : cela changerait le défaut global.
- Piloter le menu `/model` au clavier (flèches puis `s`, pour la session seule) : on dépendrait de
  l'affichage de Claude Code, que les hooks ne décrivent pas.
- Choisir le modèle dans « Nouvel agent » : un clic de plus à chaque lancement.

## Tickets

- #59 — Agents — choisir le modèle et l'effort d'un agent depuis le panneau Session
