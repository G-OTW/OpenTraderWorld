# Dashboard et navigation

L'écran d'accueil de l'application, plus les deux éléments qui se trouvent au-dessus de chaque module : la zone de recherche et la boîte de notifications.

## Pages du dashboard

Le dashboard s'ouvre sur une page intégrée **Modules** : une tuile par module installé, reconstruite automatiquement quand vous installez et détachez. Elle n'est jamais modifiée ni supprimée ; elle reflète simplement ce que vous avez.

En plus de cela, vous créez **vos propres pages**. Chacune a un nom, une description optionnelle, et un court **tag** affiché sur sa pastille. Une page est la page **par défaut** : celle sur laquelle s'ouvre le dashboard, et celle dont la pastille est triée en premier.

Utilisez-les comme se découpe une journée de trading : une page *Matin* avec le flux d'actualités, le calendrier économique et la checklist de routine ; une page *Positions* avec le portefeuille et la watchlist ; une page *Admin* avec les tâches et les minuteurs.

## Modifier une disposition

**Modifier la disposition** transforme une page en grille de lignes sur 12 colonnes. En mode édition vous pouvez :

- **ajouter des lignes** et y déposer des **tuiles de module** (un lien vers un module, et le même module peut apparaître sur autant de pages que vous voulez) ou des **widgets** ;
- **redimensionner** toute tuile par étendue de colonnes, et faire glisser les tuiles entre les lignes ;
- définir le **préréglage de hauteur** d'un widget (compact, standard ou haut) et ouvrir sa **config** (l'engrenage sur la tuile) ;
- insérer des **lignes d'espacement** pour aérer entre les blocs.

Les tuiles sont des liens, pas des copies : en retirer une d'une page ne touche jamais le module ni ses données.

## Widgets

Un widget est un aperçu vivant et interactif d'un module : il lit et écrit via l'API propre à ce module, donc ce que vous faites dans le widget est réel. Les widgets dont le module n'est pas installé ne sont simplement pas proposés.

| Widget | Ce qu'il fait |
|---|---|
| **Free text** | Une note ou un titre que vous écrivez vous-même, en markdown allégé. |
| **News feed** | Derniers éléments d'un flux choisi, en liste ou en grille. |
| **Mailbox** | Les derniers mails non lus, du plus récent au plus ancien. |
| **Time tracker** | Démarrer/arrêter un minuteur de projet sans quitter la page. |
| **Quick trade** | Choisir une catégorie + un modèle et ouvrir le formulaire d'ajout de trade. |
| **Goals** | Une courte liste d'objectifs avec progression ; en ajouter un directement. |
| **ToDo** | Tâches ouvertes, cochables sur place. |
| **Trading routine** | La checklist du jour, cochable sur place. |
| **Mindset** | Le bilan du jour. |
| **Reminder** | Un formulaire rapide d'ajout de rappel. |
| **Calendar** | Aujourd'hui et cette semaine d'un coup d'œil. |
| **Economic calendar** | Événements macro à venir, condensés. |
| **Portfolio** | Un résumé de portefeuille avec valeur en direct. |
| **Subscriptions** | Dépense récurrente mensuelle, puis ce qui se renouvelle ensuite. |
| **Net worth** | Valeur nette actuelle, sa variation sur une fenêtre que vous définissez, et une sparkline. |
| **Watchlist** | Cotations en direct d'une liste choisie : prix, variation 24h et 7j. |
| **Fundamentals** | Séries et tableaux macro, un aperçu de société, une ligne d'état financier par trimestre, année ou TTM, sociétés triables, dépôts filtrés, résultats à venir et valorisation face aux pairs stockés. Lu depuis les données stockées sans consommer de quota fournisseur. |
| **Quant** | Jeux de données et backtests disponibles, risque et drawdown d'un actif, corrélation, saisonnalité, volatilité réalisée face à sa plage historique, et régime de marché estimé. |
| **Prompt store** | Vos prompts par tag, cliquez sur l'un pour le copier. |
| **Resources** | Favoris d'une catégorie choisie. |
| **Agent** | Interroger l'assistant : choisir le modèle et les outils, envoyer, arriver dans la conversation. |

Les widgets Fundamentals et Quant s'actualisent toutes les cinq minutes tant que la page est visible. Ils conservent leur résultat précédent pendant l'actualisation et expliquent une mise à jour échouée. Fundamentals ne lit que des instantanés stockés ; chargez ou mettez à jour les données manquantes sur la page du module correspondant.

Dans les **réglages du widget**, choisissez un jeu de données Quant ou un panier de deux à vingt jeux de données compatibles. Les membres d'un panier doivent partager une unité de temps ; les membres intraday doivent aussi partager un fournisseur. Le risque propose une confiance de VaR historique de 90 %, 95 % ou 99 %, la saisonnalité propose rendements, volatilité, volume ou amplitude de barre, et les cartes de volatilité et de régime exposent leur fenêtre ou leur nombre d'états. Chaque analyse affiche son historique réel et la taille de son échantillon.

Les réglages de Fundamentals permettent de choisir et d'ordonner les séries d'un tableau macro, de choisir jusqu'à quatre métriques de société ou colonnes de tableau, de sélectionner des sociétés pairs, et de filtrer les dépôts et résultats sur les sociétés suivies. Le TTM d'un état financier additionne quatre trimestres consécutifs et est disponible pour les lignes de compte de résultat et de flux de trésorerie ; les valeurs du bilan restent des observations de fin de période. Une comparaison d'une année sur l'autre exige la même période fiscale l'année précédente et une base de comparaison positive.

Survolez, mettez en focus ou touchez une cellule de heatmap pour inspecter la valeur et sa taille d'échantillon. Les cellules manquantes sont hachurées, distinctes d'un zéro mesuré. Les cartes étroites montrent des résumés de saisonnalité mensuelle ou les plus fortes corrélations de paires. Les liens des widgets ouvrent l'onglet du module concerné avec la série, le jeu de données ou le panier sélectionné.

Sur les écrans de téléphone jusqu'à 480 px de large, les cartes du dashboard s'empilent en une seule colonne. La disposition enregistrée reste disponible sur les écrans plus larges et dans l'éditeur de disposition.

## Recherche globale

La zone de recherche de la barre du haut, accessible de partout avec <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, ou simplement <kbd>/</kbd> quand vous ne tapez pas dans un champ.

Par défaut elle cherche parmi les **noms de modules**, les **sections des Paramètres** et les entrées de **Resources**. Le **bouton de couches** à côté de la zone l'élargit à votre contenu : pages Editor, Goals, événements Calendar, ToDos, Routines, Reminders, Prompts et Community Docs.

Deux choses qu'elle ne fait volontairement pas : elle ne cherche que dans les **titres et noms, jamais dans les contenus**, et elle ne cherche que dans les modules que vous avez installés. Les résultats reviennent groupés par type, correspondances de préfixe d'abord ; <kbd>↑</kbd>/<kbd>↓</kbd> et <kbd>Entrée</kbd> permettent de naviguer.

## Notifications

La cloche de la barre du haut porte un compteur de non lus et ouvre la **boîte de notifications**, où arrivent les rappels [RemindMe](/fr/modules/productivity#remindme) quand ils se déclenchent, ainsi que tout ce qu'un [webhook](/fr/modules/productivity#webhooks) entrant y redirige. Une notification qui se déclenche pendant que vous êtes dans l'application glisse aussi en bannière.

La livraison par **e-mail, Telegram, Slack ou Discord** passe par les [canaux de notification](/fr/config/settings#notifications) partagés dans les Paramètres, où vous décidez aussi quels modules peuvent pousser vers chacun. La boîte elle-même est toujours active et n'exige aucune configuration.
