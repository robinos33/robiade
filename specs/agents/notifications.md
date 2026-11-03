# Notifications

**En une phrase** : hors de la fenêtre, un agent qui m'attend se signale par une notification
macOS et le badge du Dock.

## Comportement

Aussi exposé hors fenêtre : badge du Dock et notification macOS sur les niveaux 1 et 2.

Notification d'un agent (issue #9) : envoyée quand son status passe à `AwaitingPermission`,
`AwaitingInput` ou `Failed` et que la fenêtre n'a pas le focus ; une seule par transition.
- titre : `projet · nom de l'onglet`, le projet seul si l'onglet n'a pas de nom ;
- corps : la raison — « attend ta permission », « attend ta réponse » (notification d'inactivité),
  « a fini son tour », « s'est arrêté sur une erreur ». Rien du transcript ;
- clic : l'ADE passe au premier plan sur l'agent, dans l'onglet Agents. L'id de l'agent voyage dans
  les données de la notification, et l'ADE vérifie qu'il est encore là avant de l'ouvrir.
- seule : l'agent ne reçoit aucune variable `ITERM_*` ni `IT2_*` du terminal d'où l'ADE a été lancé.
  Sans elles, le hook global `cc-status` d'iTerm ne sait pas à quelle session s'adresser, et les
  sessions lancées dans iTerm gardent leurs notifications.

## Tickets

- #9 — Notifications : remplacer celles d'iTerm2
