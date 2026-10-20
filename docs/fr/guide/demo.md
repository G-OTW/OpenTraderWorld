# Mode démo

Il existe un bac à sable public sur **[demo.opentraderworld.com](https://demo.opentraderworld.com)** : la vraie application, remplie de données d'exemple, partagée par tout le monde, et réinitialisée toutes les **15 minutes**. Rien à installer, rien à créer ; vous arrivez connecté en tant que `demo`.

Cette page explique ce qu'est ce mode, pour que vous sachiez ce que vous regardez, et pour que vous puissiez en lancer un vous-même si vous voulez montrer l'application à quelqu'un.

## Ce que c'est

Le mode démo est une posture que le backend adopte lorsqu'il démarre avec `OTW_DEMO=1`. **Il n'est jamais activé implicitement** : une installation normale n'est pas touchée par tout ce qui suit.

- **La base de données est réinitialisée au quart d'heure.** Le jeu de départ est restauré depuis un modèle, donc tout ce que vous modifiez disparaît à :00, :15, :30 ou :45. La bannière de l'application décompte jusqu'à la prochaine.
- **Vous êtes connecté automatiquement** avec le compte `demo`. Il n'y a pas de mot de passe à deviner, ni de compte à créer.
- **Tout le monde partage une seule base.** Les trades, notes et conversations des autres visiteurs sont visibles, et les vôtres le sont pour eux. Ne saisissez rien que vous ne publieriez pas.

## Ce qui est bloqué

Le filtre est en **refus par défaut** : une requête doit correspondre à une liste d'autorisation explicite, sinon elle est refusée avec `demo_disabled`. Une route à laquelle personne n'a pensé est fermée, pas ouverte, la même règle que suit le [catalogue MCP](/fr/config/ai-agents).

En gros :

| Bloqué | Lecture seule | Complet |
|---|---|---|
| Configuration initiale, déconnexion, compte et mot de passe, réseau, sauvegarde, mise à jour, effacement des données, installation/détachement de modules, **le coffre**, **l'Automator**, webhooks entrants, le point d'entrée MCP lui-même, canaux de notification, installation de FinanceDatabase, téléchargements auprès des fournisseurs | Data connectors, flux, fichiers, portefeuilles de gérants, paramètres et tokens MCP, débit API, jeux de données stockés, fournisseurs d'agents, mémoires et compétences | Journal, backtest, quant, portefeuilles, calendrier, tâches, objectifs, éditeur et bases de données, prompts, ressources, abonnements, taxcalc, temps, dashboard, recherche, chat de l'agent |

Vous pouvez donc consigner un trade, lancer un backtest et parler à l'assistant ; vous ne pouvez pas changer le mode réseau, créer un token, prendre la base de données, ni faire télécharger la machine auprès d'un fournisseur facturé à l'usage pour votre compte.

Deux d'entre eux méritent un mot, car le module est visible mais ne fait rien :

- **Le coffre** est fermé d'emblée. C'est le seul stockage dont tout le but est de contenir des identifiants, et cette base est partagée et publique.
- **L'Automator** est fermé par la règle de refus par défaut plutôt que par une ligne qui lui est propre : un workflow atteint tout ce que son token autorise et peut appeler n'importe quelle URL, exactement ce qu'un bac à sable public ne doit pas offrir. Ouvrir le module affiche l'interface ; chaque requête derrière répond `demo_disabled`.

Les messages de chat sont aussi limités à 2000 caractères.

## Quotas par visiteur

Les points d'entrée coûteux coûtent de l'argent réel, donc chacun porte **deux** budgets : une part par visiteur, et un plafond global par-dessus. Le plafond par IP seul ne limiterait pas la dépense ; le plafond global seul laisserait un visiteur scripté bloquer tout le monde.

| | Par visiteur | Sur toute la démo | Fenêtre |
|---|---|---|---|
| **Exécutions de l'agent** | 3 | 8 | 10 minutes |
| **Exécutions de l'agent** | 10 | 40 | 24 heures |
| **Backtests et balayages** | 10 | 30 | 10 minutes |

L'assistant tourne sur une **clé partagée figée sur un modèle gratuit**, résolue au démarrage ; la mémoire à long terme et les serveurs MCP externes sont désactivés.

## Lancer la vôtre

```bash
OTW_DEMO=1        # in the core service's environment
otw-core --seed-demo   # once, against a scratch database
```

Le jeu de départ est public : il est livré dans le dépôt, ne contient aucun secret, et s'arrête immédiatement si un utilisateur `demo` existe déjà. La réinitialisation est une restauration `CREATE DATABASE … TEMPLATE` pilotée depuis l'hôte, pas par l'application.

::: warning Ne pointez pas le mode démo sur vos données
Le jeu de départ écrit dans ce que nomme `DATABASE_URL`, et la réinitialisation restaure par-dessus. Utilisez une base jetable.
:::
