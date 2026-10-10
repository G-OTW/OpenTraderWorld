# Vue d'ensemble des modules

OpenTraderWorld est un ensemble de **modules**, des blocs de fonctionnalités que vous activez individuellement dans **Paramètres → Modules**. Tout est livré avec l'application ; installer un module le fait apparaître dans le sélecteur de modules (en haut à gauche) et sur le dashboard. Détachez un module pour le masquer de nouveau (ses données sont conservées sauf si vous les supprimez aussi).

Le [dashboard, la recherche et les notifications](/fr/modules/dashboard) se trouvent au-dessus de tous les modules et sont toujours là.

## Dépendances

Certains modules s'appuient sur le catalogue de jeux de données de **Historical Data** et exigent qu'il soit installé :

```
Historical Data ──▶ Historical Data Visualization
                ──▶ Backtest
                ──▶ Quant Tools
```

Tout le reste est indépendant, même si certains modules s'intègrent quand les deux sont installés (p. ex. Tax Calculator peut importer le PnL du Trading Journal ; MyWealth peut importer les avoirs de Portfolio Tracker ; Calendar peut afficher les ToDos, les Goals et les Reminders).

Historical Data, Visualization, Watchlists, Fundamentals et le Trading Journal partagent aussi une seule liste de **[data connectors](/fr/config/connectors)** : un compte de fournisseur est créé une fois et accordé aux modules qui peuvent l'utiliser.

## Tous les modules

### Trading

| Module | Ce qu'il fait |
|---|---|
| [Trading Journal](/fr/modules/journal) | Journal de trades avec modèles, barèmes de frais, change multi-devises et statistiques de performance. |
| [Trading Routines](/fr/modules/productivity#routines) | Listes de contrôle de session récurrentes : préparation pré-marché, discipline en séance, revue post-marché. |
| [Mindset](/fr/modules/productivity#mindset) | Bilans quotidiens d'humeur et de discipline avec tendances. |

### Données de marché et analyse

| Module | Ce qu'il fait |
|---|---|
| [Historical Data](/fr/modules/market-data#histdata) | Télécharge l'historique OHLCV auprès de plusieurs fournisseurs dans des jeux de données locaux. |
| [Historical Data Visualization](/fr/modules/market-data#histviz) | Un espace de graphiques chandeliers/OHLC/lignes/Renko avec indicateurs, dessins, comparaisons et alertes surveillées côté serveur, en direct ou à la demande, sur tout instrument qu'un connector sert. |
| [Backtest](/fr/modules/market-data#backtest) | Backtester de stratégies à base de règles avec dimensionnement, coûts et statistiques complètes. |
| [Quant Tools](/fr/modules/market-data#quant) | Risque, statistiques, volatilité et régimes d'un jeu de données ; paires, paniers et régression factorielle ; tests de surajustement sur les backtests enregistrés ; dimensionnement, calculateurs, courbes de futures et surfaces de volatilité des options. |
| [Fundamentals](/fr/modules/fundamentals) | Séries macro, états financiers des sociétés, dépôts SEC, transcriptions, ETF, calendrier de marché et données alternatives issues de sources primaires et des agrégateurs que vous connectez. |

### Portefeuilles et argent

| Module | Ce qu'il fait |
|---|---|
| [Watchlists](/fr/modules/portfolio#watchlists) | Listes de symboles avec prix en direct, variations du jour, sparklines et notes. |
| [Portfolio Tracker](/fr/modules/portfolio#portfolios) | Valeur en direct, registre de cash et de revenus, performance au regard du risque pris, dérive d'allocation et stress tests. |
| [MyWealth](/fr/modules/portfolio#wealth) | Valeur nette de tout ce que vous possédez et devez, avec des portefeuilles lus en direct plutôt que copiés. |
| [Managers' Portfolios](/fr/modules/portfolio#mportfolios) | Positions 13F des superinvestisseurs, consultables et sauvegardables en instantanés. |
| [Tax Calculator](/fr/modules/portfolio#taxcalc) | Estimations fiscales de trading et d'investissement à partir de modèles par pays. |
| [Subscriptions](/fr/modules/portfolio#subscriptions) | Abonnements récurrents et vue d'ensemble des dépenses. |

### Actualités et recherche

| Module | Ce qu'il fait |
|---|---|
| [News](/fr/modules/news-research#news) | Agrégateur d'actualités RSS et API JSON avec dashboards interrogés périodiquement. |
| [Mailbox](/fr/modules/news-research#mailbox) | Newsletters, actualités de marché et mails de broker lus depuis votre propre boîte IMAP, sans trackers. |
| [Economic Calendar](/fr/modules/news-research#economics) | Événements macro à venir. |
| [FinanceDatabase](/fr/modules/news-research#findb) | Recherche locale parmi plus de 300 000 instruments ; organisez vos favoris en dossiers. |
| [Resources](/fr/modules/news-research#resources) | Bibliothèque de favoris pour livres, liens et références. |
| [Community Docs](/fr/modules/news-research#community-docs) | Guides écrits par la communauté, synchronisés et lisibles hors ligne. |

### Notes et organisation

| Module | Ce qu'il fait |
|---|---|
| [Editor](/fr/modules/productivity#editor) | Éditeur de documents enrichi avec dossiers et bases de données table/kanban/galerie. |
| [ToDo](/fr/modules/productivity#todos) | Liste de tâches avec échéances et catégories. |
| [Goals](/fr/modules/productivity#goals) | Objectifs avec suivi de métriques et échéances. |
| [Calendar](/fr/modules/productivity#calendar) | Calendrier d'événements personnel ; superpose rappels, tâches et objectifs. |
| [RemindMe](/fr/modules/productivity#remindme) | Rappels avec notifications dans l'application et canaux e-mail/Telegram/Slack/Discord. |
| [Time Tracker](/fr/modules/productivity#time) | Minuteurs de projets avec budgets et valeur au taux horaire. |
| [Prompt Store](/fr/modules/productivity#prompt-store) | Bibliothèque consultable de prompts IA réutilisables, étiquetés, notés et versionnés. |
| [Webhooks](/fr/modules/productivity#webhooks) | URL entrantes privées qui transforment des alertes externes en notifications. |
| [Automator](/fr/modules/automator) | Workflows sur votre propre API et le monde extérieur, à la main ou planifiés. |

### IA

| Module | Ce qu'il fait |
|---|---|
| [Agent](/fr/modules/agent) | Assistant de chat IA intégré (apportez votre propre fournisseur) qui peut aussi agir sur vos données via MCP, avec mémoire, compétences et serveurs MCP externes. |
