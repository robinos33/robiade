# Modèle et effort d'un agent

**En une phrase** : les lignes Modèle et Effort du panneau Session sont des listes ; un choix
relance l'agent sur sa session avec ce modèle et cet effort, sans toucher aux réglages de Claude
Code.

## Comportement

- **Les listes sont celles de Claude Code** : les modèles de son écran `/model`, avec ses libellés
  et ses descriptions, et pour l'effort les niveaux que le modèle visé accepte (celui choisi, sinon
  l'entrée « défaut »). Elles dépendent du compte et de la version de Claude Code, et sont
  demandées à la CLI une fois par exécution de Robiade. Un modèle sans niveau d'effort grise la
  liste Effort ; un effort que le nouveau modèle n'accepte pas est abandonné au changement.
- L'entrée « défaut » ne passe aucun flag : le réglage de Claude Code s'applique. Sans choix,
  l'effort en service nomme le défaut (« high (défaut) ») ; le modèle en service est dans la barre
  de jauges, au-dessus du terminal.
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
- Seules les valeurs que Claude Code propose partent en arguments de `claude` : la commande vérifie
  le choix sur la liste. Un choix enregistré se relit sans elle, pourvu qu'il reste un argument sûr
  (court, sans espace, jamais en tête `-`) : il est repassé tel quel, même si la liste ne le
  propose plus.
- La liste se demande sans appel à l'API ni trace : `claude -p` en protocole `stream-json`, requête
  `initialize`, session non enregistrée, hooks coupés. Elle est demandée depuis le dossier
  personnel : les réglages de projet (`availableModels`) n'y comptent pas. Illisible, les listes
  sont grisées avec l'erreur au survol, et la demande se retente à l'affichage suivant.
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

Décidé le 2026-11-10 : **les listes viennent de Claude Code**, pas d'une liste en dur (issue #59,
à la demande de Robin). **Risque résiduel** : la requête `initialize` est celle de l'Agent SDK
(`supportedModels()`), sans contrat publié pour la CLI ; si son format change, les listes se
grisent.

## Écarté

- Taper `/model X` ou `/effort X` dans le PTY : cela changerait le défaut global.
- Piloter le menu `/model` au clavier (flèches puis `s`, pour la session seule) : on dépendrait de
  l'affichage de Claude Code, que les hooks ne décrivent pas.
- Choisir le modèle dans « Nouvel agent » : un clic de plus à chaque lancement.
- Une liste de modèles en dur : elle se périme à chaque sortie de modèle et ignore le compte.

## Tickets

- #59 — Agents — choisir le modèle et l'effort d'un agent depuis le panneau Session
