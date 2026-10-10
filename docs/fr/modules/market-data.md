# Données de marché et backtesting

Ces modules forment une chaîne : **Historical Data** télécharge l'historique de prix dans des jeux de données locaux ; **Backtest** et **Quant Tools** travaillent sur ces jeux de données, et **Visualization** trace n'importe quel instrument qu'un connector sert, stocké ou non. Les trois exigent que Historical Data soit installé, puisqu'il possède le catalogue de jeux de données qu'ils lisent.

## Historical Data {#histdata}

Téléchargez des bougies OHLCV depuis des fournisseurs externes dans des jeux de données stockés dans votre base, ou [importez un fichier](#import) que vous avez déjà. Une fois stockées, les données sont à vous : tracez-les, backtestez-les, exportez-les, sans re-téléchargement.

### Fournisseurs et identifiants

Les fournisseurs sont configurés une fois, de façon centralisée, sous forme de **[data connectors](/fr/config/connectors)** : un compte de fournisseur nommé avec ses identifiants, une limite de requêtes optionnelle, et les modules autorisés à l'utiliser. Historical Data n'a **aucun réglage de fournisseur propre** : le bouton *Connectors* à côté du sélecteur de fournisseur ouvre le même écran partagé que celui des Paramètres.

Certains fournisseurs sont **sans clé** (Binance et Binance futures, Bitget, OKX, Kraken, Coinbase, Yahoo Finance) et fonctionnent immédiatement ; les autres exigent une clé API, et la plupart ont des offres gratuites. Un connector auquel il manque ses identifiants affiche *identifiants requis* et est ignoré jusqu'à ce que vous les renseigniez.

Les appels sortants sont comptés dans **Paramètres → Débit API** pour que vous puissiez surveiller votre usage de l'offre gratuite.

### Téléchargement

Choisissez fournisseur, type d'actif, unité de temps, ticker et plage de dates, puis **Télécharger**. Notes :

- Les **futures** utilisent des codes de contrat : base + lettre du mois + chiffre de l'année (`F G H J K M N Q U V X Z` = janv…déc), p. ex. `GCJ5` pour l'or d'avril 2025.
- Les **options** sont construites à partir du sous-jacent, de l'échéance, call/put et du strike.
- **Limites intraday** : les fournisseurs ne servent la granularité intraday que sur une profondeur limitée (p. ex. ~7, 60 ou 730 jours selon le fournisseur). L'historique plus ancien est disponible en **1d / 1w** sans limite. Le formulaire vous avertit avant de mettre en file une plage impossible.

::: warning Jeux de données téléchargés avant la v0.0.15
Avant la v0.0.15, un téléchargement ou une mise à jour qui atteignait l'heure courante pouvait stocker la bougie encore en formation, et les mises à jour suivantes démarraient après elle. Un tel jeu de données peut contenir des **bougies incomplètes** (high, low, close et volume tronqués). Retéléchargez-le pour les remplacer. Depuis la v0.0.15, une bougie n'est stockée qu'une fois sa période terminée.

| Fournisseur | Jeux de données pouvant contenir des bougies incomplètes |
|---|---|
| Coinbase, Kraken, Yahoo Finance, Alpaca, Massive, EODHD, Alpha Vantage, Capital.com, Interactive Brokers | téléchargés ou mis à jour avant la v0.0.15 |
| Binance (spot) | téléchargés ou mis à jour avant la v0.0.12 |
| Binance USDⓈ-M, Bitget, OKX, OANDA, TradeStation, FOREX.com | aucun |
:::

### Plusieurs à la fois

Un seul formulaire met en file tout un lot : cochez **autant d'unités de temps** que nécessaire et saisissez **plusieurs tickers séparés par des virgules** (`BTCUSDT, ETHUSDT, SOLUSDT`). Un téléchargement est mis en file par couple symbole × unité de temps (3 symboles × 2 unités de temps = 6 téléchargements), tous sur le même connector et la même plage de dates.

Avant d'appuyer sur Télécharger, le formulaire **chiffre le lot** : combien de téléchargements, environ combien de requêtes au fournisseur cela coûte, et le temps minimum que cela prendra (ils s'exécutent l'un après l'autre pour respecter les limites de débit du fournisseur). Quand le connector porte une [limite de requêtes](/fr/config/connectors#request-limits), une petite jauge montre la part de la fenêtre courante déjà dépensée, et la ligne vous avertit quand le lot la dépasse. Ce n'est pas bloqué : le reste attend la réinitialisation du quota et reprend de lui-même.

### Suivre les tâches

Les téléchargements s'exécutent comme des **tâches** d'arrière-plan, tronçon par tronçon, avec une progression en direct. Filtrez les tâches par statut, fournisseur, unité de temps ou ticker ; un lot est groupé sous un en-tête montrant combien de ses téléchargements sont terminés.

- **Une tâche qui a atteint une limite est `waiting`, pas en échec.** La ligne dit pourquoi (*quota atteint* ou *limite de débit du fournisseur*) et décompte jusqu'au moment où elle reprend d'elle-même.
- **Annuler** arrête toute tâche non terminée, et un clic annule **le reste d'un lot**. L'annulation est coopérative : le worker s'arrête à la prochaine limite de tronçon et les barres déjà écrites sont conservées.
- Une longue pause et la fin d'un lot déclenchent une notification, poussée vers les [canaux](/fr/config/settings#notifications) accordés à Historical Data.

### Jeux de données

L'onglet **Datasets** liste tout ce qui est stocké : nombre de barres, plage de dates, taille. De là vous pouvez :

- **Récupérer les plus récentes** : récupère les barres plus récentes que la dernière stockée (compléter un jeu de données).
- **Exporter** en **CSV** ou **Parquet**. Le CSV s'ouvre dans n'importe quel tableur ; le Parquet est les mêmes barres typées et compressées, environ un dixième de la taille, lu par `pd.read_parquet` sans analyse de dates ni devinette de types. Le fichier Parquet porte aussi l'instrument, l'unité de temps et la source dans ses propres métadonnées, donc le réimporter n'importe où dans l'application remplit le formulaire tout seul.
- **Supprimer** un jeu de données (supprime toutes ses barres).
- Aller directement à un **graphique** de celui-ci.

Les téléchargements Capital.com et OANDA stockent aussi le **bid et l'ask** de chaque bougie ; pour les autres fournisseurs un backtest les récupère quand il [en a besoin](#bid-ask-providers).

Les jeux de données importés figurent dans la même liste, nommés d'après l'origine de leur fichier plutôt que d'après un fournisseur. Ils n'ont pas de bouton *Récupérer les plus récentes* : il n'y a pas de fournisseur derrière eux, et le moyen d'en étendre un est un autre fichier.

### Importer votre propre fichier {#import}

**Import** dans l'onglet Datasets lit un historique de prix que vous avez déjà : un dump d'exchange, un export de broker, un tableur, l'archive d'un éditeur. CSV, TSV, TXT, JSON ou **Parquet**, jusqu'à 20 Mo, une ligne par barre.

Le fichier ne quitte jamais votre navigateur entre les étapes et n'est jamais stocké sur le serveur : chaque étape le renvoie, donc il n'y a pas de téléversement à moitié fini à reprendre ou nettoyer.

**Les colonnes sont proposées, vous les confirmez.** Les en-têtes sont comparés à un dictionnaire multilingue (six langues) *et* à l'aspect réel des valeurs, donc `Date;Ouverture;Plus haut;…` et `open_time,open,high,low,close,volume` se mappent tous deux. Un point à côté de chaque colonne indique la confiance du détecteur ; ce dont il n'est pas sûr vous est laissé. Corriger une colonne lui apprend : le fichier suivant avec cet en-tête se mappe tout seul.

Un fichier Parquet est lu dans la même grille qu'un CSV, donc la détection des colonnes, l'étape de mappage et l'aperçu fonctionnent à l'identique. Ses types sont respectés : un horodatage INT96 hérité (ce qu'écrivent Spark et les anciens pandas) et un `DATE` deviennent des dates, un `DECIMAL` garde son échelle, un null reste une cellule vide. Le même lecteur sert les imports du Journal et de Portfolio, qui acceptent donc aussi le Parquet.

Les **horodatages** sont lus comme des dates ou des entiers Unix en secondes, millisecondes, microsecondes ou nanosecondes, détectés par colonne et modifiables. Un horodatage texte sans fuseau est lu au décalage que vous choisissez, ce qui décide de *la période* à laquelle appartient chaque ligne, pas seulement de son affichage.

**Ce qui manque est rempli, jamais inventé.** Un fichier avec une seule colonne de prix est une série de clôtures (une VL, un niveau d'indice) : open, high et low sont remplis depuis la clôture, faisant une barre plate, et le formulaire le dit. Une colonne que vous *avez* mappée et qui est vide sur une ligne est une erreur nommant sa ligne, pas un zéro.

Avant que quoi que ce soit soit écrit, l'aperçu rapporte sur tout le fichier : barres, lignes, colonnes, première et dernière date, l'espacement réel de vos horodatages (proposé comme unité de temps), périodes manquantes à cet espacement, lignes partageant une période, barres dont high/low ne contiennent pas open/close, et chaque ligne illisible.

**Ce que le fichier ne peut pas dire, vous le dites.** Un fichier dit "Close" ; il ne dit pas que les barres sont AAPL en journalier. L'import demande donc :

- **Ticker**, **type d'actif** et **unité de temps** (l'unité de temps est pré-remplie depuis l'espacement du fichier).
- **Source** : le broker, la place ou l'éditeur d'où vient le fichier, en texte libre. Elle fait *partie de l'identité de la série*, donc le même instrument exporté par deux brokers reste deux jeux de données au lieu de deux rubans moyennés en un.
- **Nom** et **tags** : vos propres étiquettes, utilisées pour retrouver la série dans le catalogue et la filtrer.

**Importer deux fois est sans danger.** Un réimport atterrit sur le même jeu de données et écrase période par période : même fichier, même résultat. Les périodes qui se chevauchent sont comptées dans l'aperçu avant que vous validiez.

Un fichier Parquet exporté d'ici saute l'essentiel de ce formulaire : il connaît déjà son ticker, son type d'actif, son unité de temps et sa source, et ne remplit que les cases que vous avez laissées vides, donc ce que vous avez saisi l'emporte. Exportez, modifiez dans pandas, réimportez.

Une fois importée, la série est un jeu de données ordinaire : backtests, Quant Tools, enrichissement du journal et proxys de facteurs du portefeuille la lisent comme n'importe quelle série téléchargée.

### Regarder avant de télécharger

Vous n'avez pas à mettre une tâche en file pour savoir si un symbole vaut d'être stocké. Le graphique récupère une fenêtre via un connector et **ne stocke rien** ; quand la fenêtre vous convient, **enregistrez-la** et la tâche de téléchargement normale est mise en file pour exactement cette plage. Enregistrer par-dessus des barres que vous détenez déjà consolide au lieu de dupliquer, donc compléter un jeu de données depuis le graphique est sans danger.

## Historical Data Visualization {#histviz}

Le graphique n'est pas lié à un jeu de données : il ouvre un **instrument**. Cherchez un symbole, choisissez une unité de temps, et les barres arrivent que vous les ayez téléchargées ou non : le serveur sert ce qui est déjà dans votre catalogue et ne récupère que les bords manquants via un [connector](/fr/config/connectors). Rien n'est écrit sauf si vous le demandez.

La page est un **espace de travail** : une grille de graphiques, une liste d'instruments à côté, et une session de backtest rapide en dessous. Tout ce qui suit décrit un seul graphique sauf mention contraire ; la grille elle-même est dans [Espaces de travail](#workspaces).

### Trouver un instrument

Rien n'est écrit par-dessus les bougies qui ne leur appartienne : le **symbole en haut à gauche d'un graphique est un bouton**, et il ouvre le sélecteur d'instruments comme un modal, une seule zone de recherche sur tous les connectors que le graphique est autorisé à utiliser. Tapez `BTC` et Binance, Bitget, OKX, Kraken, Coinbase, Yahoo, EODHD, Alpha Vantage, Alpaca et Massive répondent ensemble, chaque résultat étiqueté avec le connector qui l'a servi. Filtrez par type d'actif ; cochez ou décochez les sources dans le même modal (la case **est** l'autorisation, et le serveur refuse un connector qui n'a jamais été accordé à ce module).

Quand la zone est vide, le panneau liste ce que vous avez tracé **récemment**, puis ce qui est déjà **stocké**, les deux s'ouvrant en un clic et sans rien coûter.

Une ligne que vous ne pouvez pas tracer le dit à la place du graphique, en nommant la raison : le connector n'a jamais été accordé, le fournisseur ne connaît pas le symbole, un identifiant manque, ou aucun connector accordé ne le sert. Chaque message porte le bouton qui règle le problème.

### Espaces de travail {#workspaces}

Un espace de travail est une **grille de graphiques**, de 1x1 jusqu'à 3x4. Les lignes et colonnes se choisissent depuis la barre d'outils, donc une séparation verticale, une horizontale et un 2x2 sont le même contrôle plutôt qu'une liste de dispositions nommées ; glissez les séparateurs pour donner plus de place à un graphique, ou agrandissez un graphique et revenez à la grille. Gardez autant d'espaces de travail que vous voulez, nommez-les, et basculez depuis le sélecteur ; celui que vous aviez ouvert revient au rechargement.

Chaque graphique porte son propre instrument, unité de temps, style de tracé, indicateurs et dessins. Un graphique se ferme depuis son propre en-tête, et une cellule vide demande un instrument.

**Groupes de liaison.** Cliquez sur le bouton de liaison d'un graphique pour lui donner une couleur. Les graphiques qui partagent une couleur partagent le **symbole** et le **réticule**, et aussi la plage visible quand ils sont sur la même unité de temps. L'unité de temps elle-même n'est volontairement jamais partagée : trois volets sur un symbole en 1m, 1h et 1d est la raison même de les lier.

**Une connexion pour tous.** Les volets sur le même compte partagent un seul flux en direct, donc quatre graphiques sur une clé Alpaca dépensent une place de connexion, pas quatre.

### La liste d'instruments {#rail}

Un rail à gauche de la page, issu de deux sources qui ne se mélangent jamais :

- Les **listes de graphiques** sont celles du rail lui-même, construites avec le bouton `+`, qui ouvre le même sélecteur d'instruments. Elles contiennent des coordonnées de graphique, donc un clic les trace sans recherche. **Recent** est la même chose sans nom.
- Les **listes du module Watchlists** contiennent des symboles de cotation, donc en tracer un est une recherche. Quand la liste ou la ligne cote via un data connector, ce connector est le seul interrogé : un symbole coté via IBKR se trace sur IBKR ou pas du tout.

Cliquez sur une ligne pour la tracer dans le volet actif, ou glissez-la sur n'importe quel volet. **On n'écrit jamais dans une watchlist d'ici** : en modifier une la copie d'abord dans une liste de graphiques et modifie la copie, et transformer une liste de graphiques en vraie watchlist est le bouton **Promouvoir** et rien d'autre.

### Charger l'historique

Le graphique s'ouvre sur les **1500 dernières barres** et place un bouton au bord gauche des données chargées. Chaque clic remonte d'une tranche. Aucune requête au fournisseur n'a jamais lieu sans un geste de votre part, ce qui rend une clé à usage mesuré prévisible ; la remontée s'arrête quand l'historique du fournisseur s'épuise.

Le menu déroulant **unité de temps** propose chaque taille de barre que le connector prend en charge, pas seulement celles que vous avez téléchargées, et changer d'unité de temps **conserve les dates que vous regardiez**.

### Live streaming {#live}

Pour un connector capable de streaming, le graphique **passe en direct tout seul** dès que la dernière barre
est celle qui se forme maintenant : la dernière bougie se met à jour sur place, et le contrôle montre l'état
de la connexion et le retard sur l'exchange. Le même contrôle arrête et redémarre le flux à la main.

Le direct est adressé par **instrument**, pas par jeu de données, et **ne stocke rien**. Tout symbole qu'un
fournisseur de streaming sert peut être regardé en direct sans le télécharger d'abord, ce qui est le but :
vous pouvez regarder quelque chose avant de décider que cela vaut d'être conservé. Enregistrez l'instrument pendant qu'il est
en direct et le même flux commence aussi à enregistrer les barres closes dans le jeu de données.

**Qui diffuse quoi.** Binance (spot et futures), Bitget, OKX, Kraken et Coinbase diffusent chaque
unité de temps qu'ils téléchargent, le journalier compris, car sur un marché 24h/24 7j/7 la bougie journalière est le jour
d'époque. Capital.com diffuse aussi chaque unité de temps, à partir des bougies bid et ask qu'il publie
séparément, tracées à leur mid. Alpaca, Massive et
Interactive Brokers publient **un seul grain chacun** (barres d'une minute, agrégats d'une minute,
barres de cinq secondes) et le graphique en dérive votre unité de temps. Cela couvre chaque unité de temps **intraday**
et laisse le journalier et l'hebdomadaire au téléchargement : une séance actions n'est pas 1440
minutes alignées sur l'époque, donc une bougie journalière construite ainsi divergerait de celle stockée. Sur
une unité de temps qui ne peut pas diffuser, le contrôle dit lesquelles le peuvent au lieu de disparaître. Le
détail par fournisseur est dans [live streaming](/fr/config/connectors#live-streaming).

Un graphique diffuse un symbole quel que soit le nombre de volets : plusieurs volets sur un instrument, et
plusieurs volets sur un compte, partagent la connexion plutôt que de prendre chacun une place.

**Quel compte.** Quand un fournisseur détient plus d'un connector, le contrôle de direct gagne un sélecteur
de compte. Deux clés sont deux droits et deux places de connexion, donc celle qui est dépensée
est votre choix, pas un repli. Changer redémarre le flux sur l'autre.

#### Pas de trou à la jointure

Charger la fenêtre, ouvrir le socket et attendre que le fournisseur publie prennent tous du temps, et
un fournisseur de barres d'une minute ne parle qu'une fois par minute. Le temps que le premier tick en direct arrive, le
graphique peut avoir une ou plusieurs bougies de retard, et ces bougies manquaient auparavant jusqu'au rechargement.

Le flux dit donc au serveur où le graphique s'arrête, et le serveur envoie ce qui s'est clos entre-temps :
depuis le catalogue quand l'instrument est stocké, depuis le fournisseur seulement pour la queue qu'il ne peut pas
servir, rien du tout quand il n'y a pas de trou. Ce que vous voyez en passant en direct est ce que le marché a
fait, sans trou à la jointure.

#### Quand il ne peut pas fonctionner

Une clé rejetée, une offre sans streaming, un instrument auquel votre compte n'est pas abonné :
ce sont des réponses, pas des pannes. Le graphique nomme laquelle c'est, cite les propres mots du fournisseur, et
**s'arrête** plutôt que de se reconnecter indéfiniment derrière un point qui ne passe jamais au vert.

| Ce que vous voyez | Ce que cela signifie | Quoi faire |
|---|---|---|
| *Not authorized* / clé rejetée | les identifiants sont faux, ou l'offre n'a pas de flux en direct (une clé Massive gratuite télécharge l'historique et est refusée à la connexion en direct) | corrigez le connector, puis appuyez sur *Réessayer* |
| *No subscription* pour ce symbole | le compte est connecté mais n'a pas droit au flux de cet instrument (Alpaca gratuit diffuse IEX, pas SIP ni OPRA ; IBKR sert ce à quoi vous êtes abonné) | choisissez un autre instrument ou ajoutez l'abonnement chez l'éditeur |
| *Connection taken* | la plupart des éditeurs autorisent une connexion en direct par compte, et un autre programme tient la place | fermez l'autre programme ; celui-ci continue de réessayer seul, puisque cela se résout de lui-même |
| *This timeframe does not stream* | le fournisseur publie un seul grain et votre unité de temps est au-dessus | passez à l'une des unités de temps que liste le contrôle, ou restez sur les données téléchargées |
| *Connector not granted* | le graphique n'a jamais reçu ce connector | accordez-le dans [Data connectors](/fr/config/connectors), depuis le lien du message |
| *Market data lines* presque toutes utilisées | Interactive Brokers plafonne le nombre de symboles qu'un compte diffuse à la fois, et l'espace de travail approche ce plafond | fermez un volet, ou arrêtez le flux en direct d'un volet que vous ne regardez pas, avant que le graphique suivant ne se taise sans raison donnée |

Tout le reste (un socket coupé, un hoquet du fournisseur) se reconnecte discrètement avec un backoff.

### Graphique

- **Types de graphique** : chandeliers, barres OHLC, ligne, et **Renko** (avec taille de brique).
- **Indicateurs** : SMA, RSI, MACD et plus, en superposition ou dans des volets séparés, chacun avec source, couleurs de ligne/remplissage et épaisseur configurables. Chaque série a **sa propre ligne dans l'en-tête du graphique**, portant masquer, réglages et retirer au survol, et chaque volet est titré au-dessus du dessin qu'il contient. L'en-tête **se lit au réticule** : O H L C, la variation, et la valeur de chaque indicateur à la barre sous le curseur, retombant sur la barre visible la plus récente quand le curseur est ailleurs.
- **Réglages du graphique** : échelle linéaire ou logarithmique, lignes de grille horizontales et verticales, **séparateurs de jours**, la **clôture précédente** tracée comme ligne de référence, réticule et ses étiquettes de valeur par série, infobulle au survol (désactivée par défaut), couleurs hausse/baisse, échelle de prix à gauche ou à droite, et une **étiquette de dernier prix** épinglée sur l'axe des prix, teintée comme la bougie qui l'a produite.
- **La navigation se fait à la main** : glissez pour déplacer les deux axes (temps latéralement, prix verticalement), molette pour zoomer (calibrée par appareil, donc un cran de souris et un tick de trackpad bougent autant). Changer de type de graphique conserve le zoom actuel.
- Le **volume** est tracé au bas du volet des prix, comme le font les terminaux de marché, plutôt que dans un volet à part. Plus un mode plein écran unique.
- Les **prix de micro-capitalisations** s'écrivent `0.0₅4549`, l'indice comptant les zéros, au lieu d'une échelle d'étiquettes `0.0000` identiques.

### Comparer deux instruments

Ajoutez un autre instrument au même graphique et il est tracé **rebasé**, puisque deux prix dans deux devises sur un axe ne disent rien. Deux lectures, à un clic d'écart :

- **variation en pourcentage**, les deux séries rebasées au début de la fenêtre, qui répond à *lequel a le plus monté* ;
- **ratio**, cet instrument divisé par l'autre, rebasé à 100, qui est la vue pair trade : la ligne monte quand celui que vous tracez surperforme.

Les séries de comparaison sont alignées sur le graphique **période par période**, donc deux marchés qui horodatent différemment le même jour s'alignent quand même.

### Enregistrer le graphique

- **En image** : un PNG du graphique exactement tel qu'il est à l'écran, dessins et superpositions compris.
- **En page** : un fichier HTML qui s'ouvre hors ligne dans n'importe quel navigateur, contenant l'image, ce dont elle est l'image (instrument, fenêtre, unité de temps, indicateurs, comparaisons, qui a servi les barres), et **les barres elles-mêmes**, intégrées. Une capture d'écran collée dans un document est une affirmation que personne ne peut vérifier plus tard ; celle-ci peut être relue. Rien n'est téléversé : le fichier est construit dans votre navigateur.

### Indicateurs personnalisés sur le graphique {#custom-indicators-on-the-chart}

La boîte de dialogue des indicateurs a un second onglet : **Custom**. Il contient la même bibliothèque de graphes de nœuds que celle avec laquelle le module [Backtest](#strategies-and-custom-indicators) construit, donc un indicateur existe **une seule fois** et les deux modules voient la même définition. Choisissez-en un dans la liste pour le tracer, ou construisez-en un nouveau ici avec le même constructeur ; l'enregistrer le réécrit dans la bibliothèque partagée.

Contrairement à un indicateur du catalogue, un indicateur personnalisé **choisit son propre volet** : sur le prix, ou dans un volet à part. Ce choix est le vôtre par instance, donc le même indicateur peut se superposer aux bougies sur un graphique et se placer en dessous sur un autre. Un indicateur 0-100 tracé en superposition reçoit une seconde échelle cachée, pour qu'il ne puisse pas aplatir le prix.

La définition **voyage avec l'instance** : un graphique trace encore son indicateur personnalisé après la suppression de la ligne de la bibliothèque, et recharger l'actualise depuis la bibliothèque tant que la ligne existe.

### Outils de dessin

Un rail contre le bord gauche du graphique : **ligne de tendance**, ligne **horizontale** et **verticale**, **rectangle**, **retracement de Fibonacci**, **texte**, boîtes de position **long** et **short** (entrée, objectif et stop, avec le R:R qui en résulte), et une **mesure** qui rapporte la variation de prix, le pourcentage, le nombre de barres et le temps écoulé.

- Chaque objet a son propre **style** (couleur, épaisseur, tirets) et se modifie en glissant ses poignées.
- Un **aimant OHLC** accroche une poignée à l'open, au high, au low ou à la clôture de la barre qui est dessous, et une poignée déposée près d'un objet déjà sur le graphique s'y accroche, avec une ligne guide disant ce qu'elle a attrapé.
- **Modèles de style** : stylez un objet, enregistrez-le sous un nom, et appliquez-le aux suivants. Un modèle peut devenir le défaut de chaque nouveau dessin.
- **Copier entre graphiques** : <kbd>Ctrl/⌘+C</kbd> puis <kbd>Ctrl/⌘+V</kbd> colle l'objet sélectionné, sur un autre instrument aussi ; <kbd>Ctrl/⌘+D</kbd> le duplique sur place, décalé d'une barre.
- Les dessins sont ancrés en **temps et prix**, pas en pixels, donc ils restent sur leurs barres à travers tout zoom, déplacement ou changement d'unité de temps.
- Ils sont conservés **par instrument**, pas par unité de temps ni par volet : une ligne de tendance tracée sur le 1h est la même ligne sur le 15m, et deux volets sur un symbole montrent un seul tableau.
- **Annuler** (le bouton du rail, ou <kbd>Ctrl/⌘+Z</kbd>) reprend le dernier dessin, ou le dernier ordre du backtest rapide.

#### La liste d'objets

Passé cinq dessins un graphique a besoin d'une liste, donc il y en a une : chaque objet de cet instrument avec ce que c'est et le prix où il se trouve, et les quatre choses dont il a alors besoin, **masquer**, **verrouiller**, **supprimer** et **réordonner**. L'ordre est l'ordre de peinture, qui décide de ce qui est dessus. Sélectionner une ligne la sélectionne sur le graphique, et la liste est le même tableau que celui que trace le graphique, donc une modification se voit avant la fermeture de la boîte de dialogue.

### Alertes {#alerts}

Un prix ou un niveau d'indicateur, **surveillé par le serveur**. Le navigateur peut être fermé, la machine peut faire autre chose : l'alerte se déclenche quand même, dans vos notifications et dans les [canaux](/fr/config/settings#notifications) accordés au graphique (aucun coché = tous).

Définissez-en une depuis la boîte de dialogue des alertes, ou depuis une ligne horizontale que vous avez déjà tracée, qui transmet son prix.

Trois règles méritent d'être connues, car ce sont des décisions plutôt que des détails :

- **Barres closes uniquement.** Le high d'une bougie en formation n'est pas encore un fait, il peut être révisé par le tick suivant. Une alerte qui se déclencherait dessus rapporterait quelque chose qui n'a jamais eu lieu.
- **Un franchissement, pas un état.** *Croise à la hausse* attend que le prix **traverse** le niveau en montant, donc une alerte placée sous le prix actuel ne se déclenche pas à l'instant où vous la créez.
- **Le niveau est lu sur l'unité de temps de ce graphique**, et une alerte journalière est relue bien moins souvent qu'une alerte à une minute : un instrument non stocké coûte une petite requête par contrôle, et une barre qui bouge une fois par jour ne mérite pas une par minute.

Chaque alerte peut se déclencher **une fois** ou à chaque fois, avec un délai de réarmement. La liste dit quand chacune s'est déclenchée pour la dernière fois, ce qu'elle a lu en dernier, et, quand un connector ou un quota gêne, pourquoi elle n'a pas pu s'exécuter du tout. Les alertes sont mises en pause et réarmées depuis cette liste.

### Broker book {#broker-book}

Synchronisez un [compte broker](/fr/config/brokers) et le graphique trace ce que vous détenez réellement : une ligne de prix par position ouverte à son coût moyen, une par ordre en cours à sa limite ou son stop. Les niveaux atterrissent sur le graphique dont le ticker correspond, ponctuation mise à part, donc un espace de travail de plusieurs instruments s'annote tout seul. En lecture seule, et relu uniquement quand vous appuyez sur *Sync* : une position sans coût moyen n'a pas de ligne et est comptée comme telle au lieu d'être placée quelque part de plausible.

### Backtest rapide

Un brouillon pour trader un graphique à la main : **cliquez sur le graphique pour ouvrir une position, cliquez de nouveau pour la fermer**. Un appui long permet à la place de choisir le sens et la taille, et cliquer sur la flèche d'un marqueur l'inverse. Pyramidage, clôtures partielles et renversements découlent tous des exécutions que vous placez.

La session couvre **tout l'espace de travail, pas un graphique** : chaque graphique à l'écran y poste ses exécutions et les chiffres en sont la somme, comme se lit réellement un book de plusieurs instruments. Le sélecteur du panneau nomme le **graphique cible**, celui sur lequel un clic place un trade et celui que modifie la case de taille ; c'est le volet actif, donc choisir ici et cliquer là sont le même acte.

La bande sous l'espace de travail est une ligne de chiffres de session quand elle est repliée. Dépliée, elle est redimensionnable par son bord supérieur et a trois onglets :

- **Trades** : la liste des trades de la session avec entrée, sortie, P&L et R (mesuré par rapport à la pire perte ouverte du trade, puisqu'il n'y a pas de stop à citer), plus une réinitialisation.
- **Statistics** : taux de réussite, espérance, profit factor, résultats par sens, séries et une distribution des rendements.
- **Performance** : la courbe de P&L de la session, chaque trade portant son run-up et sa pire perte ouverte, donc un gagnant qui a passé la journée dans le rouge se lit comme tel.

La taille se saisit en **unités**, **contrats** (multipliés par une valeur du point) ou **notionnel**, et se convertit au prix d'exécution. Ces exécutions vivent **dans votre navigateur, par instrument** : c'est un brouillon pour lire un graphique, jamais des données de journal, et rien n'est posté dans le [Trading Journal](/fr/modules/journal).

Le bouton **Backtest** transmet le même instrument au module [Backtest](#backtest), en l'enregistrant d'abord s'il n'était pas stocké.

### Le graphique se souvient d'où vous l'avez laissé

Le type de graphique, les indicateurs et les dessins appartiennent à l'**instrument**, pas à un jeu de données ni à un volet : ils sont enregistrés côté serveur sous les propres coordonnées du symbole peu après chaque modification, et reviennent de la même façon dans n'importe quel volet, n'importe quel espace de travail, depuis n'importe quel navigateur. Cela vaut pour un symbole que vous n'avez regardé qu'une fois et jamais téléchargé, ce qui est le but : tracer quelque chose que vous n'avez pas décidé de conserver ne signifie plus perdre ce que vous avez dessiné dessus.

Ce qui n'est *pas* stocké côté serveur est la session de backtest rapide, qui reste dans ce navigateur.

L'interrupteur **autosave data** dans les réglages du graphique est une décision distincte, sur les barres plutôt que sur la disposition : il stocke un instrument la première fois que vous le tracez, ce qui met un téléchargement en file. Il est désactivé par défaut.

### Quand des données manquent

Une fenêtre qui revient courte dit toujours **pourquoi**, dans un avis au-dessus du graphique, tout en gardant les barres qui sont arrivées :

| Raison | Ce qui s'est passé |
|---|---|
| **auth** | L'identifiant du connector est manquant ou rejeté. |
| **quota** | Le connector a atteint sa propre [limite de requêtes](/fr/config/connectors#request-limits). |
| **rate_limit** | Le fournisseur a bridé la requête. |
| **symbol** | Le fournisseur ne connaît pas ce ticker. |
| **depth** | Le fournisseur ne sert pas l'historique aussi loin à cette unité de temps. |
| **provider** | Tout autre retour du fournisseur. |

## Backtest {#backtest}

*Combinez des signaux d'indicateurs, dimensionnez avec pyramidage, mesurez l'avantage.* Choisissez un jeu de données ou tout un portefeuille, définissez des règles, lancez. Sans code.

### Stratégie

- **Règles d'entrée / de sortie** par sens, construites à partir de comparaisons entre indicateurs, prix et valeurs fixes. Groupez les règles avec **AND** (toutes doivent tenir) ou **OR** (une seule suffit).
- **Direction** : long, short, ou les deux. Options : dériver le côté short comme le miroir du long (opérateurs inversés, niveaux d'oscillateurs en miroir : RSI sous 30 devient RSI au-dessus de 70 ; filtres ADX, ATR et volume conservés tels quels), et **stop & reverse** (inverser la position quand le signal opposé se déclenche).
- **Stop-loss / take-profit** par sens : chacun est une case que vous activez ou désactivez indépendamment (pourcentage de l'entrée moyenne, ou multiple de l'ATR de la dernière bougie close avant l'entrée). Sans règles de sortie, les sorties se font via SL/TP ou renversement.

### Dimensionnement, compte et coûts

- Dimensionnez en **pourcentage de l'équité** ou en **quantité/lots/contrats fixes**, avec **levier** et **capital de départ**. La quantité fixe **évolue avec le levier**, selon la convention retail, donc un levier de 3 sur une taille fixe de 1 ouvre 3 unités.
- **Pyramidage** : autorisez jusqu'à N entrées empilées quand le signal d'entrée se redéclenche ; SL/TP suivent alors le prix d'entrée moyen. Un ajout est envoyé à l'ouverture, avant que la bougie soit testée : une bougie qui touche ensuite le stop clôt la position que l'ajout a faite, et un stop que l'ajout a déplacé (breakeven) est testé sur cette même bougie.
- **Coûts** : frais (fixes ou % du notionnel, par trade ou par unité) et **spread %**, pour que les résultats ne soient pas fantaisistes. Des frais peuvent être négatifs, pour une remise maker ou un broker qui paie par exécution. Les frais d'entrée sortent du cash à l'exécution, comme un broker les débite, donc l'équité, le drawdown et les ajouts d'une position ouverte sont nets de ceux-ci.
- Les réglages sans sens (pas de capital, une taille ou un levier de zéro ou moins, un contrat sans valeur, une grille impossible à trader) sont refusés avant l'exécution, de même qu'un jeu de données contenant une bougie qui n'en est pas une (un high sous le low, un prix qui n'est pas un nombre), avec cette bougie nommée.

### Dimensionnement (avancé)

Au-delà du pourcentage de l'équité et de la quantité fixe :

- **Risque par trade** : dimensionne pour qu'un stop-loss touché coûte un % fixe de l'équité (exige un stop sur le sens tradé).
- **Kelly fractionnaire** : dimensionne à partir du taux de réussite et du payoff des *N* derniers trades signalés de la stratégie, ignorés compris (un trade à l'équilibre n'est ni un gain ni une perte), multiplié par la fraction choisie et plafonné ; une taille d'échauffement est utilisée jusqu'à ce que la fenêtre se remplisse. Une période perdante met les entrées en pause sans les arrêter définitivement : elles reprennent quand l'avantage revient.
- **Paliers d'équité** : une table de seuils ; le palier le plus haut dont le niveau est ≤ l'équité actuelle fixe la taille.

### Portefeuille (multi-actifs)

Ajoutez plusieurs jeux de données et exécutez une stratégie sur tous sur une **horloge fusionnée** (tous verrouillés sur la même unité de temps) :

- Un **aperçu d'alignement** montre la longueur de l'horloge fusionnée, la fenêtre de chevauchement, les barres d'échauffement des indicateurs (y compris le recul cumulé d'un indicateur personnalisé enchaîné), et les barres manquantes par actif, avant même que vous simuliez.
- **Limites de portefeuille** : plafonnez le nombre de positions ouvertes et l'exposition totale / par actif. Une position détenue à une ouverture garde sa place là même si elle se clôt plus tard dans cette bougie.
- **Séances** : les actifs dont les bougies s'ouvrent à des heures différentes (un jour crypto à 00:00 UTC, un jour New York à 13:30) agissent dans cet ordre. Un ordre à l'ouverture la plus précoce dimensionne et vérifie ses limites sur la clôture précédente de l'actif plus tardif, pas sur une ouverture qui n'a pas encore eu lieu.
- Une **ventilation par actif** rapporte trades, PnL net, frais, taux de réussite et exposition pour chaque instrument.

### Stratégie de grille

Une échelle de niveaux de prix entre une borne basse et une borne haute ; chaque cellule achète bas et vend au niveau suivant, **long**, **short** ou **neutre**. Dimensionnez une quantité fixe par niveau ou répartissez un budget total entre les cellules, avec des stops optionnels au-dessus/en dessous de l'échelle. Les résultats rapportent exécutions, allers-retours et inventaire final.

- **Neutre** trade les deux côtés de la ligne centrale : les cellules en dessous achètent et vendent un niveau au-dessus, les cellules au-dessus vendent à découvert et rachètent un niveau en dessous. Il faut un nombre impair de niveaux.
- Un achat ne repose que sur une ligne **sous** la dernière clôture (une vente au-dessus), et s'exécute à la ligne, ou à l'ouverture quand la bougie s'ouvre au-delà. Les objectifs s'exécutent de la même façon.
- Un **stop** sous ou au-dessus de l'échelle s'exécute à son niveau (à l'ouverture sur un gap), après les exécutions que le prix a rencontrées en chemin, puis arrête la grille.
- Sans bornes, l'échelle couvre la plage connue jusque-là : le plus bas et le plus haut des bougies précédant chacune.
- Hors de la fenêtre de trading avec *close*, l'inventaire part à l'ouverture, avant tout autre événement de la bougie.

### DCA (plan d'épargne)

Un troisième mode à côté des règles de signaux et de la grille, pour la façon dont l'argent est réellement investi le plus souvent : un **panier pondéré**, acheté dans le temps, jamais rééquilibré.

- **Pondérations fixes.** Chaque euro déployé est réparti selon les pondérations que vous fixez par ticker. Rien n'est rééquilibré, donc une règle qui se déclenche sur un actif sur cinq déploie la part propre de cet actif.
- **Argent entrant.** Le capital de départ est acheté d'un coup à la première barre de chaque actif. Tout ce qui suit est de l'**argent frais** : un apport récurrent (par barre, jour, semaine, mois, trimestre ou année, investi à l'arrivée ou gardé en cash), et des règles d'achat qui versent un montant quand leur condition tient.
- **Règles d'achat** : un montant fixe, un % du cash, du portefeuille ou du prix de revient, avec un nombre maximal de déclenchements et un délai de réarmement, exécutées à l'ouverture de la barre suivante.
- **Règles de vente** : un % de la position, toute la position, un nombre d'unités ou un montant, déclenchées par un **objectif de gain**, une condition, ou les deux, avec le produit gardé en cash ou retiré.
- Les **conditions** sont les groupes de règles ordinaires du moteur plus deux familles écrites pour ce mode : les **métriques de marché** (baisse depuis le plus haut, hausse depuis le plus bas, variation sur N barres, variation depuis le début) et la **position en cours** (P&L %, dérive depuis le dernier achat, coût moyen, unités, valeur, poids %, cash %, drawdown). Elles sont évaluées par défaut sur un **indice de panier pondéré**, ou par actif, qui n'achète alors que les actifs qui tiennent.
- **Des mesures pour un plan d'épargne**, pas pour une stratégie : drawdown et Sharpe sur la courbe **corrigée des dépôts (pondérée par le temps)**, pour qu'un dépôt ne soit pas lu comme une hausse ; rendement sur l'argent investi ; **TRI** pour le rendement pondéré par l'argent ; et un benchmark du même total apporté déployé en une seule fois au début.

Dimensionnement, pyramidage et stops ne s'appliquent pas ici : les règles propres du plan décident de chaque exécution.

### Fenêtre de trading

Une étape **Filters** décide *quand* la stratégie peut ouvrir, sur l'horloge que vous choisissez : un fuseau horaire nommé (`America/New_York`), qui suit l'heure d'été, ou un décalage UTC fixe, qui ne la suit pas :

- **Jours de semaine** et **séances** (plusieurs par jour, une fin avant son début passe minuit).
- **Calendrier** : ne trader que certaines dates, ou jamais ces dates. *Jamais* l'emporte sur *seulement*.
- Hors de la fenêtre la position est soit **conservée**, soit **clôturée**, et les ajouts de pyramidage peuvent aussi être bloqués. Les entrées sont filtrées ; les sorties, stops et take-profits continuent de tourner à chaque barre.

### Coûts et réalisme d'exécution

- **Slippage** : un nombre fixe de ticks ou un pourcentage du prix, appliqué à chaque exécution.
- **Funding** : un taux annuel constant sur le notionnel ouvert pour les estimations de perp (les longs paient, les shorts reçoivent).
- **Coupe-circuits** : arrêtent le trading après une perte quotidienne maximale (pour la journée) ou un drawdown maximal (pour l'exécution).
- **Profil d'instrument** : tick de prix, pas de lot, quantité minimale et multiplicateur de contrat, pour que tailles et prix s'alignent sur un contrat réaliste. Chaque exécution et chaque stop, objectif, limite et ligne de grille est arrondi sur le tick, contre le trader : un achat paie le tick au-dessus, le stop d'un long se place un tick plus bas.

### Exécution des ordres {#execution}

Comment les stops et objectifs s'exécutent, toujours :

- Un trade est testé face à son stop et son take profit **sur la bougie où il s'ouvre**, pas à partir de la suivante.
- Le **take profit est un ordre à cours limité** : il s'exécute à l'objectif, sans slippage ni spread facturé, dès que le côté qui traite l'atteint (le bid pour un long, l'ask pour un short), ou seulement quand le prix **traverse** l'objectif si vous choisissez cela (Avancé, *Exécution*). Une bougie qui **s'ouvre au-delà de l'objectif** l'exécute à cette ouverture, avant tout autre événement de la bougie.
- Le **stop loss est un ordre stop** : il s'exécute au stop, ou à l'ouverture quand la bougie le dépasse en gap, et paie spread et slippage.
- Quand une bougie atteint **à la fois** le stop et l'objectif et que rien d'autre ne dit lequel est venu en premier, le stop l'emporte.
- Un trade clôturé dans une bougie (stop, objectif, limite) n'est pas rouvert à l'ouverture de cette bougie, un prix d'avant la sortie : une nouvelle entrée attend la bougie suivante.
- Tout signal décidé à une clôture est exécuté à **l'ouverture suivante** : une entrée, la condition de sortie, et *sortir quand l'entrée ne tient plus*. Rien ne s'exécute à la clôture qui a produit sa propre décision.
- MAE et MFE ne comptent que ce que le trade a vécu : de son exécution à sa sortie, jamais le reste de la bougie après son départ.

Options de l'étape **Advanced** (*Exécution*), toutes désactivées par défaut :

- **Ordre d'entrée** : au marché, ou **limite**. Une limite repose sous la référence pour acheter et au-dessus pour vendre, à un décalage (pourcentage, distance de prix ou multiple d'ATR) de la clôture de la bougie du signal ou de l'ouverture de la bougie suivante. Elle reste valide le nombre de bougies que vous fixez, s'exécute au toucher ou seulement quand le prix la traverse, et s'exécute à son propre prix sans slippage (à l'ouverture quand une bougie s'ouvre au-delà). Un signal qui continue de tenir ne déplace pas un ordre en attente. Les ajouts de pyramidage suivent la même règle.
- **Ordre de sortie** : le même choix pour les sorties sur signal (la condition de sortie, et *sortir quand l'entrée ne tient plus*). Une limite de sortie qui ne s'exécute pas à temps soit **passe au marché** à l'ouverture suivante, soit est **annulée**. Un stop-and-reverse reste un renversement au marché.
- **Vérifier l'unité de temps inférieure pour SL/TP** : quand une bougie atteint à la fois le stop et l'objectif, ou qu'une limite s'exécute en milieu de bougie, l'exécution lit une unité de temps inférieure du même instrument (même fournisseur) à l'intérieur de *cette bougie seule* pour voir ce qui est venu en premier. Elle lit d'abord un jeu de données stocké à cette unité de temps, puis des bougies téléchargées par une exécution antérieure, qui sont conservées pour les exécutions suivantes. *Auto* prend l'unité de temps stockée la plus fine, sinon la plus fine que le fournisseur sert en une requête par bougie. Les bougies d'unité inférieure qui ne correspondent pas à la bougie (extrêmes différents) ne sont pas utilisées.
- **Télécharger les bougies d'unité inférieure manquantes** : avec cette option, les bougies dont une exécution a besoin et qu'elle ne détient pas sont téléchargées auprès du fournisseur, et seulement celles-là. Après une exécution, un avis dit combien de bougies se sont réglées en stop faute de celles-ci, avec le coût (requêtes et temps) et une case pour les récupérer. Un téléchargement jusqu'à environ 4 minutes démarre de lui-même dans une fenêtre de progression ; un plus long vous attend, avec la limite de débit du fournisseur et le quota restant sur son connector, car il peut échouer s'il n'en reste pas assez. L'exécution rejoue ensuite avec les bougies téléchargées. Quand le fournisseur refuse (pas de permission ou d'abonnement pour ces données, une clé, une limite de débit) ou échoue trois fois de suite, l'exécution cesse de le solliciter et le dit à côté des résultats. Une session paper avec l'option télécharge les bougies sous la bougie qui vient de clore quand elle en a besoin, en attendant un instant que le fournisseur les publie ; sans l'option, ou quand elles ne viennent jamais, le stop l'emporte.
- **Utiliser les prix bid/ask** : les exécutions lisent le bid et l'ask au lieu du mid et du spread (un achat au marché paie l'ask, le stop et l'objectif d'un long se déclenchent sur le bid). Ils sont récupérés auprès du fournisseur uniquement pour les bougies où une exécution peut avoir lieu, de la façon la plus simple qu'il offre (voir le tableau ci-dessous), et conservés pour les exécutions suivantes. Une courte récupération démarre d'elle-même après l'exécution, une longue demande d'abord, avec la même fenêtre de progression que l'unité de temps inférieure. Une bougie sans bid/ask utilise le spread, et le résultat dit combien. Les fournisseurs sans bid/ask historique laissent l'option désactivée et disent pourquoi.
- **Frais maker séparés** (dans le bloc Coûts) : les exécutions limite (limites d'entrée et de sortie, take profit) paient leurs propres frais, négatifs pour une remise.
- **Prix bruts** : actions et ETF sont cotés sur des barres ajustées des splits et dividendes quand le fournisseur les donne (Yahoo, EODHD, Alpaca ; les barres Interactive Brokers arrivent ajustées des splits). Cochez ceci pour exécuter sur les prix tels que tradés. Un jeu de données d'actions sans série ajustée est signalé à côté des résultats.

Le résultat montre les ordres limite placés, exécutés et expirés, combien de bougies avaient à la fois un stop et un objectif à portée et comment elles ont été réglées (unité de temps inférieure ou pire cas), et combien d'exécutions ont été cotées au bid/ask.

#### Bid/ask et prix en direct par fournisseur {#bid-ask-providers}

| Fournisseur | Bid/ask historique (backtest) | Prix en direct (stops, objectifs, limites en paper) | Bid/ask en direct (exécutions paper) |
|---|---|---|---|
| Capital.com | Bougies bid et ask | Oui | Oui |
| OANDA | Bougies bid et ask | Non, vérifié à la clôture de la bougie | Non utilisé |
| Interactive Brokers | Séries de bougies bid et ask | Oui | Oui |
| FOREX.com | Bougies bid et ask | Non, vérifié à la clôture de la bougie | Non utilisé |
| Alpaca | Cotations à l'ouverture et à la clôture de la bougie (actions, ETF, crypto) | Oui | Oui |
| Massive | Cotations à l'ouverture et à la clôture de la bougie (actions, ETF, options, forex) | Oui | Oui |
| Binance, Binance Futures, Kraken, OKX, Bitget, Coinbase | Aucun, le spread s'applique | Oui | Oui |
| TradeStation, Yahoo, EODHD, Alpha Vantage | Aucun, le spread s'applique | Non, vérifié à la clôture de la bougie | Non utilisé |

Avec des cotations lues à l'ouverture et à la clôture, le spread à l'intérieur de la bougie est leur moyenne autour du high et du low propres de la bougie.

#### Paper trading sur le prix en direct {#paper-live}

Une session paper lit ses signaux sur des bougies closes, à son unité de temps, et surveille ses niveaux sur le prix en direct entre-temps :

- Le **stop loss, le take profit et les ordres limite** (entrée et sortie) se déclenchent au premier prix en direct qui les atteint, sans attendre la clôture de la bougie. Le **stop suiveur** se déplace toujours à chaque clôture, et le prix en direct le déclenche au niveau fixé alors.
- L'exécution est cotée au bid/ask en direct quand la stratégie utilise le bid/ask, sinon au prix en direct avec le spread et le slippage des réglages. Elle est **alertée immédiatement**, et l'exécution suivante la rejoue sur sa bougie telle qu'elle s'est produite, sans seconde alerte.
- Un fournisseur sans prix en direct pour l'instrument est nommé dans la boîte de dialogue de session : là, stops, objectifs et limites sont vérifiés à la clôture de la bougie. Si le flux en direct s'arrête, ou que l'application est hors ligne, la bougie reprend la main et le journal de session le dit.
- Une entrée est **alertée à la clôture qui donne son signal**. Un ordre au marché est annoncé au prix de cette clôture, avec sa taille (prise sur l'équité valorisée à cette clôture) et son **montant** en argent pour un broker qui prend un ordre en notionnel en unités fractionnaires, puis s'exécute à l'ouverture suivante : au premier prix en direct quand le fournisseur en diffuse un (son stop et son objectif sont alors surveillés en direct à partir de cette exécution), sinon à l'ouverture de cette bougie une fois close. Le prix est mis à jour sans seconde alerte. Un ordre limite est annoncé quand il est placé (*Triggering limit order*), et son exécution est alertée quand elle a lieu.
- Une entrée limite exécutée en direct reçoit son stop et son objectif à l'exécution suivante ; d'ici là cette position est protégée à la clôture de la bougie.

Le paper trading fait tourner le même moteur que le backtest, avec les mêmes options : avec *Télécharger les bougies d'unité inférieure manquantes* ou le bid/ask activé, une exécution récupère ce dont a besoin la bougie qui vient de clore, en attendant un instant que le fournisseur la publie. La ligne de session liste les ordres limite qu'elle a en cours.

### Stratégies et indicateurs personnalisés {#strategies-and-custom-indicators}

- **Stratégies nommées** : enregistrez, cherchez, dupliquez et modifiez des configurations de stratégie complètes.
- **Versions de stratégie** (optionnel) : avec le versionnage activé dans [Paramètres → Versions](/fr/config/settings#versioning), une stratégie enregistrée peut être versionnée depuis le menu d'historique de l'en-tête. Enregistrez une version (datée, avec une note optionnelle), relisez ce que disait chaque étape, restaurez-la (l'état restauré est enregistré comme une nouvelle version, annotée avec la date de la version restaurée, et garde son nom) ou supprimez-la. L'historique s'ouvre dans une fenêtre avec une recherche sur les notes et les dates ; la version correspondant à la stratégie actuelle est marquée. Les notes sont plafonnées à 500 caractères. Les modifications non enregistrées sont enregistrées avant que la version soit prise. Désactiver le versionnage pour une stratégie demande s'il faut conserver ou supprimer ses versions. Supprimer une stratégie qui a des versions demande s'il faut les conserver ; celles conservées peuvent la restaurer depuis **Stratégies supprimées avec versions** dans l'onglet Strategies. Les agents avec accès en écriture à Backtest peuvent faire de même via [MCP](/fr/config/ai-agents), en enregistrant une version après chaque mise à jour comme un commit.
- **Indicateurs personnalisés** : construisez les vôtres à partir d'étapes nommées, sans code. Chaque étape applique soit un indicateur intégré à une **source** (un champ de prix ou la sortie d'une étape antérieure), soit calcule une **formule** référençant des étapes antérieures par leur nom (`@volume / SMA(@volume)`, avec `+ − × ÷`, `min`, `max`, `abs`, `clamp`). Cela permet d'enchaîner des indicateurs : une Hull MA d'un RSI, un MACD d'un RSI, un ratio de volume lissé, etc. Les indicateurs qui lisent des bougies complètes (ATR, Stochastique, ADX, VWAP…) ne s'appliquent qu'au prix, pas à une étape dérivée. La ou les étapes en surbrillance sont la sortie. Les indicateurs personnalisés deviennent des opérandes dans l'éditeur de règles à côté des intégrés, et la bibliothèque est **partagée avec le graphique**, qui trace la même définition ([Indicateurs personnalisés sur le graphique](#custom-indicators-on-the-chart)).

- **Sélecteur d'indicateurs avec recherche** : choisissez les indicateurs dans une liste groupée filtrable à la frappe (dans l'éditeur de règles comme dans le constructeur d'indicateurs personnalisés) au lieu de faire défiler un long menu déroulant.

### Fenêtres de dates et balayages de paramètres (API)

Deux capacités vivent dans l'API plutôt que dans le formulaire. Elles existent pour l'[assistant](/fr/modules/agent) et pour quiconque pilote l'application via [MCP](/fr/config/ai-agents) :

- **Exécutions à fenêtre de dates.** `from` / `to` sur une exécution (et sur l'aperçu d'alignement) restreignent la période simulée, ce dont ont besoin la validation walk-forward et le découpage par régimes : exécutez 2019-2021, puis 2022-2024, et comparez. `to` inclut toute la journée.
- **`POST /api/backtest/sweep`** : exécute une grille de paramètres côté serveur et renvoie **chaque essai**, avec le nombre d'essais. Les chemins de la grille pénètrent dans les tableaux (`long.entry.conditions.0.left.period`), donc les périodes d'indicateurs sont balayables ; plafonné à 4 axes et 64 essais.

Un balayage renvoie aussi un **Sharpe dégonflé** : le Sharpe que le meilleur de N stratégies *sans valeur* atteindrait vraisemblablement, vu l'ampleur de la variation de ces essais précis. Comparez le gagnant à cette barre, pas à zéro : sur de vraies barres journalières, le meilleur de huit croisements de moyennes mobiles à 0,59 face à une barre de sélection de 0,70 signifie *aucune preuve d'un avantage*, ce que le maximum seul aurait caché.

Il renvoie aussi la **probabilité de surajustement du backtest** (PBO) de la grille : les courbes d'équité des essais sont découpées en 16 blocs, chaque moitié sert une fois d'ensemble in-sample et une fois d'ensemble out-of-sample, et la PBO est la part des découpages où le gagnant in-sample se classe dans la moitié basse en out-of-sample. L'onglet [Compare](#quant) de Quant calcule le même chiffre sur des exécutions enregistrées.

### Optimiseur

Prenez une exécution terminée et **faites varier ses paramètres** : longueurs et seuils d'indicateurs par sens, stops, dimensionnement, coûts, limites de portefeuille, réglages de grille, et quels jours de semaine exclure (chaque sous-ensemble est essayé). Chaque paramètre reçoit un de / à / pas, et l'en-tête compte les variantes à mesure que vous élargissez, plafonné pour qu'une grille reste finie.

- **Avant le démarrage**, il estime le coût à partir de ce que les exécutions passées ont mesuré sur votre machine : millisecondes par variante, workers, durée totale. Vous pouvez arrêter à tout moment et garder ce qui a été calculé.
- **Classement** sur la métrique que vous choisissez (Sharpe, Sortino, rendement, profit factor, taux de réussite, espérance, drawdown max, trades) ; n'importe quelle colonne retrie ensuite. Avec un découpage out-of-sample, chaque chiffre classé est celui **in-sample**, et le rendement out-of-sample est affiché mais jamais classé : un gagnant choisi dessus l'aurait vu.
- **L'analyse** montre la dispersion de la métrique choisie sur toutes les variantes : pire, moyenne, meilleure, et combien sont sorties positives. Un seul bon chiffre ne veut pas dire grand-chose si ses voisins sont terribles.
- **Décote pour tests multiples** : mesurée sur les bougies par an que les données ont réellement (un marché 24h/24 a environ cinq fois plus de bougies horaires qu'une action), le meilleur Sharpe est montré face à la **barre de sélection**, le Sharpe que le meilleur d'autant de stratégies *sans valeur* atteindrait vraisemblablement. En dessous de la barre, essayer autant de variantes suffit à expliquer le gagnant.
- Cliquez sur une variante pour lire son **backtest complet**, rejoué depuis ses propres réglages. Rien n'est stocké tant que vous ne la **gardez** pas dans l'historique.

### Découpage out-of-sample

Découpez les données en une tête **in-sample** et une queue **out-of-sample** ; la stratégie tourne sur les deux et les deux blocs de statistiques (rendement, profit factor, taux de réussite, drawdown max, trades) sont montrés côte à côte. Un grand écart entre les colonnes est un signe de surajustement.

### Paper trading

*Une exécution terminée, laissée tourner vers l'avant.* Appuyez sur **Paper trade** sur un résultat et la stratégie continue de trader en paper, sur un calendrier, en alertant les canaux que vous choisissez. Il n'y a pas de second moteur : chaque exécution resimule la fenêtre avec le backtest ordinaire et rapporte ce qui a changé, donc une exécution paper est par construction celle que le backtest aurait montrée sur les mêmes bougies.

- **La stratégie est figée** telle qu'elle a tourné, instruments compris. Modifier cette stratégie ensuite ne change pas une session en cours ; le formulaire propre à la session propose de la mettre à jour ou de démarrer une copie quand vous enregistrez de nouveau la stratégie.
- **La première exécution amorce le book.** Chaque aller-retour déjà dans la fenêtre est enregistré d'un coup, résumé en un seul événement : ouvrir une session ne déclenche pas une rafale d'alertes sur l'historique. Seul ce qui arrive ensuite est une exécution digne d'un message.
- **Fenêtre** : combien d'historique chaque exécution fournit au moteur (bougies récentes, ou un début épinglé).
- L'onglet **Paper** liste les sessions avec leur statut, prochaine exécution, positions ouvertes et événements non lus, et chacune peut être exécutée maintenant, mise en pause, reprise ou supprimée. Le journal d'événements est conservé que quelque chose ait été envoyé ou non, donc ce qui n'a pas pu être livré est toujours là à votre retour.

#### Planification

Chaque exécution, et ses données, suit l'horloge propre de la bougie.

- **Intervalle** (toutes les N minutes), **quotidien**, **hebdomadaire**, **mensuel** ou **une fois**, dans un vrai fuseau horaire.
- Un intervalle se déclenche sur la **grille des bougies**, jamais à la seconde où la session a été créée : chaque minute à :00, toutes les 15 minutes à :00 / :15 / :30 / :45, chaque heure pile.
- L'exécution **télécharge la bougie qu'elle attend** dans les jeux de données propres à la session. La bougie en cours n'est jamais stockée : ce qui est lu est la dernière *close*, une semaine de son lundi au suivant, et un jour seulement une fois une journée complète écoulée depuis son horodatage (une bougie journalière US arrive après 04:00 UTC). Une dernière bougie stockée avant la fin de sa période est relue. Une bougie que le fournisseur n'a pas encore publiée est redemandée sur quelques secondes, et une période sans aucun échange n'écrit rien, ce qui signifie simplement que l'exécution suivante n'a rien de nouveau à simuler.
- Une exécution qui ne trouve aucune nouvelle bougie ne coûte rien : elle ne simule pas du tout.

#### Ce qui est envoyé, et où

- **Notifier** : à chaque exécution, seulement sur une nouvelle exécution d'ordre, ou jamais. *Chaque exécution* rapporte aussi les calmes, ce qui est le seul moyen de distinguer "rien ne s'est passé" de "le moteur a cessé de tourner".
- **Grouper les messages** : envoi immédiat, ou un digest horaire, quotidien ou hebdomadaire. Un envoi groupé est un message sur une période, pas un ping par exécution.
- **Alerter sur** les entrées, les sorties, ou les deux, et éventuellement seulement pour les **instruments** que vous nommez.
- **Canaux** : les [canaux de notification](/fr/config/settings#notifications) accordés à Backtest, tous ou ceux que vous choisissez. Sans canal configuré rien n'est envoyé, et chaque événement est quand même enregistré dans l'application.

#### Messages personnalisés

Trois messages, trois formulations, chacune avec son propre vocabulaire : **Entry**, **Exit** et **Summary** (le groupé). Laissez un champ vide et la formulation intégrée est utilisée.

- Chacun a un **Titre** et un **Corps**, écrits avec des marqueurs :

```
{{trade.ticker}} {{trade.direction}} at {{trade.entry_price}}
```

- **Variables** liste exactement ce que ce message peut lire, avec une valeur d'exemple ; cliquez sur l'une pour l'insérer. Une entrée lit la moitié entrée du trade, une sortie le trade entier (P&L compris), et le résumé lit la période, `since.*` (depuis la dernière alerte), `total.*` (depuis le début de la session) et `open.*` (ce qui est détenu maintenant), plus `session.*`, `event.*` et `stats.*`. Un chemin qu'un message ne peut pas lire ne lui est pas proposé.
- Les **filtres** s'écrivent `| name:arg`, et `upper`, `lower`, `trim` et `json` n'en prennent aucun :

```
{{trade.pnl | round:2}} {{since.from | date:YYYY-MM-DD}} {{trade.exit_reason | default:-}}
```

- L'**Aperçu** est le moteur de rendu propre du serveur, donc ce qu'il montre est ce qui serait envoyé. Il s'exécute sur un trade d'exemple et sur les derniers chiffres de la session, donc une valeur que la session n'a pas encore produite est marquée sur place, entre crochets, et le reste du message se rend quand même.

### Résultats

- Statistiques principales : rendement (vs **buy & hold**), PnL net et frais, taux de réussite, profit factor, espérance, drawdown max, Sharpe/Sortino.
- **Courbe d'équité** superposée au prix avec marqueurs d'entrée/sortie.
- Un **résumé de performance** complet (profit/perte brut, payoff ratio, plus gros gain/perte, nombre maximal de gains/pertes consécutifs, barres moyennes en trade…) et la **liste complète des trades** avec **MAE/MFE** par trade (pire perte ouverte / meilleur profit ouvert pendant le trade), filtrable, et motifs de sortie (signal estompé, signal de sortie, stop-loss, take-profit, renversé, fin des données).
- **Enregistrez des exécutions** par nom et gardez un historique pour comparer les stratégies plus tard. Le menu **Reports** d'une exécution terminée l'exporte en entier, par sens et par actif : un **PDF** avec chaque graphique, ou du **Markdown** avec les seuls chiffres. Aucun ne contient la liste des trades ; le fichier est nommé d'après la stratégie et le moment de l'export.

## Quant Tools {#quant}

Analyses sur vos jeux de données, vos backtests enregistrés et, pour les dérivés, directement depuis un fournisseur. Les onglets forment six groupes.

Quels onglets ont besoin de quoi :

- **Asset** et **Multi-asset** lisent des jeux de données [Historical Data](#histdata) stockés.
- **Strategy** lit des exécutions [Backtest](#backtest) enregistrées.
- **Sizing** et **Calculators** prennent des nombres que vous saisissez (le ciblage de volatilité lit aussi un jeu de données).
- **Derivatives** lit un [connector](/fr/config/connectors) Interactive Brokers ou Massive accordé à Quant.

### Asset

Un jeu de données et une fenêtre de temps, partagés par chaque onglet du groupe.

- **Risk** : volatilité historique annualisée, drawdown max, **Value at Risk** et **Conditional VaR** à votre niveau de confiance. La queue au-delà de la VaR est estimée de trois façons (normale, Cornish-Fisher, un ajustement de Pareto généralisée sur les pires pertes). Un tableau liste les pires drawdowns avec leur profondeur, leurs dates, et les barres nécessaires pour atteindre le creux et pour récupérer, car un seul chiffre de drawdown max cache le temps qu'il a fallu pour combler le trou.
- **Statistics** : quel genre de série c'est.
  - Distribution face à une normale de même moyenne et volatilité : asymétrie, excès de kurtosis, un graphique QQ.
  - Dépendance sérielle : autocorrélation des rendements et des rendements absolus, avec p-values de Ljung-Box. Les rendements en montrent rarement, les rendements absolus généralement : c'est le regroupement de volatilité.
  - Tendance ou retour à la moyenne : Hurst (R/S et DFA), ratios de variance par horizon, et demi-vie du prix.
  - Stationnarité : ADF et KPSS, lus ensemble.
  - Si le ratio de Sharpe se distingue de la chance : t-stat, Sharpe probabiliste, longueur minimale d'historique.
- **Volatility** :
  - Cinq estimateurs sur les mêmes barres. Close-to-close n'utilise que les clôtures ; Parkinson, Garman-Klass, Rogers-Satchell et Yang-Zhang lisent aussi l'amplitude de la barre.
  - Leurs trajectoires glissantes.
  - Un **cône de volatilité** qui dit si la lecture d'aujourd'hui est haute ou basse pour son horizon.
  - Une prévision **GARCH(1,1)** avec sa persistance et la demi-vie des chocs.
- **Regimes** :
  - Un modèle de Markov caché gaussien découpe les rendements en 2 à 4 états, le plus calme d'abord, et ombre le graphique de prix selon l'état le plus probable.
  - Rendement, volatilité, temps passé et séjour typique de chaque état.
  - Les probabilités de transition, et l'état dans lequel le marché est le plus probablement maintenant.
- **Events** : choisissez une condition (gap, grosse clôture, croisement SMA ou RSI, nouveau plus haut ou plus bas sur N barres, série, pic de volume) et voyez ce que le marché a fait ensuite.
  - Rendements futurs à plusieurs horizons face à la référence inconditionnelle sur les mêmes barres, avec une p-value par horizon.
  - Le chemin moyen autour de l'événement.
- **Seasonality** :
  - Une heatmap mois × jour de semaine du rendement moyen, de la volatilité, de l'amplitude de barre ou du volume, plus des bandes par mois, jour de semaine et (intraday seulement) heure.
  - Chaque cellule montre sa taille d'échantillon et son taux de réussite. L'horloge horaire est en **UTC**.

### Multi-asset

Deux jeux de données ou plus avec la même unité de temps.

- **Portfolio** : matrice de corrélation, **frontière efficiente** (un nuage d'allocations aléatoires ; cliquez sur le point Sharpe max ou volatilité min pour lire ses poids) et **risk parity**.
- **Pairs** :
  - Cointégration (Engle-Granger, et Johansen dans les deux sens) et le spread avec son ratio de couverture, z-score et demi-vie.
  - Corrélation et bêta glissants.
  - Corrélation lead-lag et causalité de Granger, pour voir si une série bouge en premier.
- **Basket** :
  - **PCA** : combien de paris indépendants le panier contient réellement.
  - Un **dendrogramme** de corrélation : qui bouge ensemble.
  - Une allocation **hierarchical risk parity**.
  - Un tableau de force relative sur 1, 3, 6 et 12 mois.
  - Un stress test qui maintient une pondération à travers chaque crise passée que couvrent les données (2008, 2020, 2022 et d'autres).
- **Regression** :
  - Les rendements d'un actif sur un ou plusieurs jeux de données de facteurs (un indice, des obligations, l'or, un secteur).
  - Alpha avec son t-stat, bêta de chaque facteur, R², tracking error et ratio d'information.
  - Capture à la hausse/à la baisse et bêta glissant.

Mélanger les classes d'actifs est possible, y compris des fournisseurs différents : les barres sont appariées par la **période** à laquelle elles appartiennent, pas par l'horodatage que le fournisseur leur a donné. Une bougie journalière crypto s'ouvre à 00:00 UTC et une d'action US au début de la séance de New York, et les deux sont le même jour. Deux choses en découlent, et le panneau dit laquelle s'est appliquée :

- Un panier qui mélange un marché 24h/24 7j/7 avec un marché à horaires de bourse est mesuré **à la semaine**. Aligné au jour, le mouvement de week-end de l'actif continu tomberait sur la même ligne que le lundi de l'autre et sous-estimerait à quel point ils bougent réellement ensemble.
- L'annualisation est **comptée sur l'horloge** plutôt que supposée : les mêmes jeux de données journaliers font 252 périodes par an sur une bourse et 365 sur un marché 24h/24 7j/7.

Les jeux de données intraday font exception : des barres 4h ancrées sur une séance de trading et des barres 4h ancrées sur l'horloge sont à 90 minutes d'écart, donc un panier intraday multi-fournisseurs est refusé plutôt qu'approximé. Utilisez des jeux de données journaliers, ou un seul fournisseur pour tout le panier.

### Strategy

Exécutions de backtest enregistrées. Chaque exécution est rejouée sur le serveur pour régénérer ses trades exacts.

- **Monte Carlo** : rééchantillonne les trades d'une exécution des milliers de fois (un par un, ou par blocs pour garder les séries ensemble).
  - **Bandes de percentiles** sur le chemin d'équité, l'équité finale et le drawdown max.
  - La probabilité de finir en perte.
  - Un **risque de ruine** : la part des chemins dont l'équité est un jour tombée à un seuil que vous fixez.
  - La vraie courbe d'équité tracée par-dessus.
- **Trades** : ce que valent les trades par unité de risque.
  - Espérance en devise et en **R**, la distribution des multiples de R, et **SQN** (sur 100 trades au plus, avec la notation de Van Tharp).
  - Le nuage **MAE/MFE** : combien de chaleur les gagnants ont encaissé, jusqu'où les perdants ont couru d'abord.
  - 1R est le stop quand l'exécution a un stop en pourcentage, sinon la perte moyenne, et la page dit lequel. Les exécutions grille et DCA n'enregistrent ni MAE ni MFE, donc le nuage est omis pour elles.
- **Compare** : 2 à 20 exécutions sur leurs dates communes.
  - Courbes d'équité rebasées, un tableau de rendement, volatilité, Sharpe et drawdown, et la corrélation de leurs rendements.
  - Le **Sharpe dégonflé** de la meilleure exécution, les autres comptant comme les essais parmi lesquels elle a été choisie.
  - La **probabilité de surajustement du backtest** (PBO, par validation croisée combinatoirement symétrique sur 8 à 16 blocs). Une PBO supérieure à 50 % signifie que le gagnant in-sample atterrit généralement dans la moitié basse en out-of-sample.

### Sizing

- **Taille de position** : à partir de votre capital, entrée, stop et risque (pourcentage ou fixe), la **taille, le notionnel, la marge, l'exposition et le reward:risk**. Il peut **suggérer des stops** à partir d'un jeu de données (volatilité, ATR, swing) et remplir l'entrée depuis la dernière clôture.
- **Kelly** : la fraction de Kelly à partir du taux de réussite et du payoff, avec demi et quart de Kelly. Le Kelly complet maximise la croissance à long terme mais oscille fortement ; la plupart des traders dimensionnent au demi ou au quart.
- **Vol targeting** :
  - Maintenez volatilité cible ÷ volatilité estimée de l'actif, l'estimation étant glissante ou EWMA, plafonnée à un levier maximum.
  - Chaque barre est dimensionnée sur l'estimation connue avant elle, donc il n'y a pas de look-ahead.
  - Montre le poids et les unités à détenir maintenant pour votre équité, et la trajectoire mise à l'échelle face à la détention plate de l'actif.
- **Risque de ruine** : la probabilité qu'un taux de réussite, un payoff et un risque par trade atteignent un drawdown donné, avec un montant fixe ou une fraction fixe risquée par trade. Trois réponses :
  - Les formes fermées : Vince, pour un montant fixe ; la borne de Cramér-Lundberg, pour l'un ou l'autre dimensionnement.
  - Une simulation sur le nombre de trades que vous fixez, avec son erreur standard et la courbe de ruine par nombre de trades.

### Calculators

- **Options** : prix et grecques Black-Scholes-Merton (vega et rho par point, theta par jour), un arbre binomial pour l'exercice américain avec la prime d'exercice anticipé, et la **volatilité implicite** d'un prix coté.
- **Base des futures** : à partir d'un prix spot et d'un prix de future, la base, le portage qu'elle implique par an, le repo implicite, la juste valeur à votre taux et rendement, et le roll yield jusqu'au contrat suivant.
- **Capitalisation** :
  - Où mènent un capital et un rendement par période, avec apports.
  - Le rendement nécessaire pour atteindre un objectif, et combien de périodes cela prend.
  - Le gain nécessaire pour remonter d'un drawdown.
- **Test de Sharpe** : pour un Sharpe cité sans ses données.
  - Se distingue-t-il de zéro, ou d'un benchmark ?
  - De quel historique a-t-il besoin ?
  - Que reste-t-il une fois le nombre de stratégies essayées pris en compte (Sharpe dégonflé, avec asymétrie et kurtosis) ?

### Derivatives

Ceux-ci lisent directement le fournisseur au lieu d'un jeu de données stocké, ils exigent donc un connector **Interactive Brokers** ou **Massive** accordé à Quant.

Une exécution représente des dizaines de requêtes au fournisseur, cadencées par le fournisseur. La page montre les requêtes effectuées sur les prévues et ce qui est en cours de récupération. Ce qui a été récupéré est conservé six heures, donc changer un taux ou une règle de roll recalcule sans redemander au fournisseur.

- **Futures curve** : les contrats à échéance d'un produit (`ES@CME` chez Interactive Brokers, `ES` chez Massive), expirés compris.
  - La **structure par terme** sur la dernière séance terminée.
  - Le **roll yield** entre le contrat proche et le suivant dans le temps.
  - Une série continue qui consiste à détenir le contrat proche et à rouler, rétro-ajustée par ratio pour finir sur le prix d'aujourd'hui, à côté de celle raccordée qui saute à chaque roll.
  - Le roll est une règle de calendrier : le contrat proche est le plus proche avec plus de *N* jours restants.
  - Avec un ticker spot (par exemple `SPX` comme indice), il ajoute la base, le portage `ln(F/S)` par an, le repo implicite, et le mispricing face à la juste valeur au taux et rendement que vous saisissez.
  - Les courbes Massive utilisent le prix de règlement de chaque séance.
- **IV surface** : la chaîne d'options d'un sous-jacent.
  - Quelques échéances réparties entre le nombre minimum et maximum de jours que vous fixez, des strikes out-of-the-money de chaque côté, chaque prix transformé en volatilité implicite Black-Scholes-Merton.
  - Sourires par échéance, **structure par terme ATM**, **risk reversal** et **butterfly** 25-delta, et un contrôle que la variance totale ATM ne diminue jamais d'une échéance à la suivante.
  - Interactive Brokers cote chaque option au dernier point médian horaire, pris à la même heure que le sous-jacent, donc aucun abonnement aux données de marché d'options n'est nécessaire. Massive cote chacune à la clôture de la séance.
  - La page vous indique le nombre de requêtes avant de commencer : sur une clé Massive gratuite (5 par minute) une chaîne prend plusieurs minutes.
- **Implied vs realized** (Interactive Brokers uniquement) : la volatilité implicite à 30 jours du sous-jacent, jusqu'à dix ans en arrière, face à la volatilité close-to-close avant et après chaque jour.
  - La **prime de volatilité** : IV moins la volatilité qui a suivi.
  - **IV rank** et **percentile** sur un recul que vous choisissez.
  - À quel point l'IV a prévu la volatilité réalisée (une régression de la réalisée qui a suivi sur l'IV).
  - La corrélation des variations d'IV avec les mouvements de prix.
