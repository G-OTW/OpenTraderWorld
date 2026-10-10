# Data connectors

Chaque module qui lit des données de marché puise dans **une liste partagée de connectors**. Un compte de fournisseur est créé une fois puis accordé aux modules qui peuvent l'utiliser.

Gérez-les dans **Paramètres → Data connectors**, sur la page autonome **/connectors**, ou depuis le bouton de connector que chaque module de données place à côté de son sélecteur de fournisseur. Les trois affichent le même écran.

## Ce qu'est un connector

Un **connector est une instance nommée d'un fournisseur**, pas le fournisseur lui-même. Quatre choses lui appartiennent :

- **le fournisseur** : Binance, Yahoo Finance, EODHD… ;
- **ses identifiants** : saisis, ou branchés depuis le [Coffre](/fr/config/settings#vault). En écriture seule : l'application ne connaît que *quels* noms de secrets sont renseignés ;
- **une limite de requêtes optionnelle** : un nombre maximal d'appels par période ;
- **les modules autorisés à l'utiliser** : un ou plusieurs, ou *tous les modules* (un joker qui couvre aussi les modules de données ajoutés dans les versions futures).

Plusieurs connectors du même fournisseur peuvent coexister. C'est le but : une clé en lecture seule pour les graphiques et une clé séparée pour les téléchargements en masse, chacune avec sa propre limite, chacune accordée à un module différent.

## Fournisseurs

| Fournisseur | Identifiants | Types d'actifs | Recherche de symboles | Flux en direct |
|---|---|---|---|---|
| **Binance** | aucun | crypto | oui | oui |
| **Binance USDⓈ-M Futures** | aucun | crypto | oui | oui |
| **Bitget** | aucun | crypto | oui | oui |
| **OKX** | aucun | crypto | oui | oui |
| **Kraken** | aucun | crypto | oui | oui |
| **Coinbase** | aucun | crypto | oui | oui |
| **OANDA** | `api_token` (+ identifiant de compte) | FX et CFD | oui | non |
| **Yahoo Finance** | aucun | actions, ETF, indices, crypto | oui | non |
| **Alpha Vantage** | `api_key` | actions, ETF, crypto, FX | oui | non |
| **EODHD** | `api_key` | actions, ETF, FX, crypto | oui | non |
| **Alpaca** | `api_key`, `api_secret` | actions, crypto, options | oui | intraday |
| **Massive (Polygon.io)** | `api_key` | actions, ETF, options, futures, crypto, FX, indices | oui | intraday, offre payante |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` | actions, ETF, options, futures, indices | par symbole | non |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | FX et CFD | oui | non |
| **Capital.com** | `api_key`, `identifier`, `api_password` | FX, indices, actions, crypto (tous des CFD) | oui | oui |
| **Interactive Brokers** | aucun (hôte + port) | actions, ETF, crypto, FX, indices, futures, options | oui | intraday |

Ceux sans clé fonctionnent dès que vous créez le connector. Chaque ligne de fournisseur renvoie vers sa propre documentation d'API et porte une note sur ses limites de débit, et un connector qui diffuse en direct porte aussi une note sur ce que coûte le direct chez lui.

### Live streaming {#live-streaming}

La portée du direct est plus étroite que celle du téléchargement, et c'est voulu.

- Les exchanges crypto publient un canal de bougies par intervalle, donc **chaque** unité de temps qu'ils téléchargent, ils la diffusent aussi, le journalier compris : sur un marché 24h/24 7j/7, la bougie journalière *est* le jour d'époque. Bitget et OKX alignent leurs propres bougies journalières et hebdomadaires sur minuit à Hong Kong, donc le téléchargement comme le flux en direct demandent leurs variantes alignées sur UTC, et une série téléchargée là-bas s'aligne avec une série téléchargée n'importe où ailleurs.
- **Trois fournisseurs ne diffusent pas ici.** OANDA et TradeStation publient les prix en direct via une réponse HTTP de longue durée, et FOREX.com via Lightstreamer ; aucun des trois n'est le WebSocket que parlent les graphiques en direct. Leurs téléchargements d'historique et leur côté compte fonctionnent ; la bougie en direct non.
- Alpaca, Massive et Interactive Brokers publient chacun un seul grain (barres d'une minute, agrégats d'une minute et barres de cinq secondes respectivement) et l'unité de temps du graphique en est dérivée. Cela rend disponibles toutes les unités de temps **intraday** et laisse le **journalier et l'hebdomadaire au téléchargement** : une séance actions n'est pas 1440 minutes alignées sur l'époque, donc une bougie journalière construite ainsi divergerait de celle que stocke le téléchargement. Le graphique le dit plutôt que de masquer le contrôle.
- Le direct se vend souvent séparément de l'historique. Une clé Massive gratuite télécharge l'historique et est refusée à la connexion en direct ; la clé gratuite d'Alpaca diffuse IEX et le flux indicatif des options mais pas SIP ni OPRA ; Interactive Brokers sert ce à quoi votre compte est abonné. Quand un flux ne peut pas tourner, le graphique nomme laquelle de ces raisons s'applique et s'arrête, plutôt que de se reconnecter derrière un point qui ne passe jamais au vert.
- La plupart de ces fournisseurs autorisent **une connexion en direct par compte**, donc un second programme sur la même clé prend la place. Ce cas est signalé comme tel et continue de réessayer, puisqu'il se résout quand vous fermez l'autre.

**Alpaca** porte un réglage pour cela : *Flux de données de marché*, `iex` (offre gratuite, par défaut) ou `sip` (payant). Il ne sélectionne que le socket en direct ; les téléchargements ne sont pas affectés.

### Les fournisseurs de dérivés crypto

`BTCUSDT` est une paire spot **et** un perpétuel, et ce sont deux séries différentes : le perpétuel s'échange avec une base par rapport au spot et un contrat à échéance converge vers lui. Donc le marché que contient un jeu de données n'est jamais déduit du ticker.

- **Binance USDⓈ-M Futures** est son propre fournisseur à côté de Binance, pas un réglage de celui-ci. Les contrats s'écrivent comme les écrit le marché des futures : `BTCUSDT` pour un perpétuel, `ETHUSDT_250926` pour un contrat à échéance. Les contrats Coin-M (inverses) ne sont pas servis.
- **Bitget** porte un réglage *Marché*, `spot` (par défaut) ou `usdt-futures`, car il écrit le même ticker à l'identique sur les deux carnets.
- **OKX** n'a besoin d'aucun réglage : ses propres identifiants d'instruments disent sur quel marché se trouve un ticker, `BTC-USDT` pour le spot, `BTC-USDT-SWAP` pour un perpétuel, `BTC-USD-241227` pour un contrat à échéance.

### OANDA

Le seul fournisseur FX avec clé ici, et sa clé est celle du compte : OANDA n'émet aucun token réservé aux données de marché, donc le connector demande le même token d'accès personnel que le [compte broker](/fr/config/brokers), plus le numéro de compte par lequel il lit les prix.

- **Réglages** : *Identifiant de compte* (`001-004-1234567-001`) et *Environnement* (`live` ou `practice`, qui sont des hôtes différents avec des tokens différents).
- **Les instruments** s'écrivent `base_quote`, indices et matières premières compris : `EUR_USD`, `XAU_USD`, `SPX500_USD`. *Tester la connexion* indique combien le compte est autorisé à coter.
- **Les bougies journalières sont calées sur minuit UTC.** Le défaut d'OANDA fait changer de jour à 17h New York, ce qui est la séance FX mais pas le jour sur lequel est stocké chaque autre jeu de données ici, donc le connector demande celui en UTC.
- Le volume est un **nombre de ticks**, pas une taille échangée : une table de négociation publie combien de prix elle a faits, pas combien a changé de mains.

Quand un fournisseur détient plusieurs connectors, le contrôle de direct du graphique gagne un sélecteur de compte : deux clés sont deux droits et deux places de connexion, donc celle qui est consommée est votre choix, pas un repli.

### TradeStation

Son périmètre de données de marché passe par la clé OAuth du compte lui-même, donc le connector demande la même paire de clés d'API et le même refresh token que le [compte broker](/fr/config/brokers). Il n'y a pas d'identifiant distinct pour les données de marché.

- **Réglages** : *Environnement* (`live` ou `sim`).
- **Les symboles** sont ceux de TradeStation : `AAPL` pour une action, `@ES` pour le future continu, `ESH26` pour un contrat, `$SPX.X` pour un indice cash, `MSFT 260116C400` pour une option. En taper un le recherche et montre ce que c'est, ce qui est le moyen le plus rapide de repérer une faute de frappe.
- **Les barres sont horodatées à leur clôture**, donc le connector soustrait l'intervalle et stocke l'ouverture, comme toute autre série ici.
- **Seuls 1m, 5m, 15m et 1d sont proposés.** TradeStation construit les barres intraday à partir de l'ouverture de la *séance*, donc sa bougie horaire commence à 9h30 et ne s'alignerait pas avec la bougie horaire de n'importe où ailleurs dans votre bibliothèque. Téléchargez 15m et lisez-le à n'importe quelle unité de temps intraday ; le refus le dit.

### FOREX.com (StoneX)

Même histoire : pas d'identifiant de données de marché propre, donc il se connecte avec les mêmes username, password et AppKey que le [compte broker](/fr/config/brokers), et les deux partagent une session.

- **Un marché est un nombre.** L'API prend un identifiant de marché numérique ; vous tapez `EUR/USD` et le connector le résout. Quand un nom correspond à plusieurs marchés, l'erreur les liste avec leurs identifiants, et vous téléchargez par identifiant.
- **Pas de volume.** Une table de négociation publie des prix, pas de taille, donc la colonne de volume vaut zéro plutôt qu'un nombre plausible.
- **Une barre journalière est la séance de la plateforme**, qui change au close de New York, pas à minuit UTC. C'est la période que StoneX a réellement échangée et elle est stockée telle quelle, donc une série journalière venue de là ne se superpose pas à celle d'un fournisseur en jour UTC.
- `4h` est refusé : StoneX ne dit pas d'où il commence à en compter une. Téléchargez `1h` et lisez-le en 4h.

### Capital.com

Le troisième avec clé dont la clé est celle du compte : Capital.com n'émet aucun identifiant de données de marché, donc le connector se connecte avec les mêmes clé API, identifiant et mot de passe personnalisé que le [compte broker](/fr/config/brokers), et les deux partagent une session.

- **Réglages** : *Environnement* (`live` ou `demo`).
- **Un instrument est un epic**, le nom de marché propre à Capital.com : `EURUSD`, `US500`, `AAPL`, `BTCUSD`. La recherche de symboles les renvoie.
- **Une bougie est le mid des deux côtés** sur lesquels la table négocie, dans le téléchargement comme sur le graphique en direct.
- **Une barre journalière est la séance de la plateforme**, pas le jour UTC, donc une série journalière venue de là ne se superpose pas à celle d'un fournisseur en jour UTC.
- **Le direct** passe par la même session et autorise 40 instruments à la fois. Capital.com diffuse le bid et l'ask comme deux bougies séparées, donc un volet se remplit une fois que les deux côtés ont tické.

### Interactive Brokers

Le cas à part : il n'y a ni URL de fournisseur ni clé API. Vous exécutez **IB Gateway** ou **TWS** sur votre propre machine et le connector parle son protocole de socket, donc ce qu'il porte est une **adresse**, pas un identifiant : un hôte et un port, stockés en clair pour pouvoir diagnostiquer une connexion échouée. Les données sont celles auxquelles votre compte IB est abonné.

- **Réglages** : *Hôte du Gateway* (`host.docker.internal` pour un gateway sur la même machine, puisqu'OpenTraderWorld tourne dans un conteneur) et *Port API* (4001 live / 4002 paper pour le Gateway, 7496 / 7497 pour TWS).
- **Dans le gateway** : Global Configuration → API → Settings, cochez *Enable ActiveX and Socket Clients*, et vérifiez que le port correspond. Sur Docker Desktop l'appel arrive depuis la boucle locale de l'hôte, donc *Allow connections from localhost only* le couvre déjà ; sur Docker Engine décochez-la et ajoutez `172.28.53.10` à *Trusted IPs*, qui accepte des adresses uniques et pas une plage.
- **Tester la connexion** indique ce qui a répondu, et nomme le réglage à changer quand rien ne répond.
- **Tickers** : `AAPL`, `SAN:EUR` ou `7203@TSEJ:JPY` pour des actions, `EURUSD` pour une paire cash, un symbole OCC pour une option. Un future s'écrit avec son mois, `ES.202512`, ou avec le symbole local que montre TWS, `MNQU6`. Les futures sont recherchés auprès du gateway avant tout téléchargement, donc la place de cotation est optionnelle : quand le ticker désigne plus d'une cotation, l'erreur les liste et vous choisissez.
- L'identifiant client est choisi par l'application dans une haute plage privée, jamais demandé, donc rien d'autre de connecté à votre gateway n'est éjecté.
- Interactive Brokers autorise 60 requêtes d'historique par 10 minutes glissantes **par compte** : le connector se cadence lui-même, donc un long rattrapage est lent par conception.

Testé avec **IB Gateway build 10.50.1e (25 août 2026)**. Les builds plus anciens devraient fonctionner, le protocole de socket étant négocié à la baisse, mais celui-ci est la version sur laquelle ce connector a été vérifié.

## En créer un

1. **Ajouter un connector**, choisissez le fournisseur, et nommez-le : le nom est ce que montrent les sélecteurs des modules, donc *Binance charts* vaut mieux que *Binance 2*.
2. Renseignez les identifiants que le fournisseur exige, ou choisissez-les dans le Coffre. Les fournisseurs sans clé sautent cette étape.
3. Choisissez les **modules** qu'il sert. Ouvert depuis un module, le nouveau connector n'est accordé qu'à ce module ; ouvert depuis les Paramètres ou `/connectors`, il est accordé à tous.
4. Définissez éventuellement une **limite de requêtes** (voir ci-dessous).

Un connector auquel il manque un identifiant requis est affiché comme *identifiants requis* et ignoré par tous les modules jusqu'à ce que vous le renseigniez.

## Autorisations des modules

La liste des autorisations est **côté serveur** : un module qui demande un connector qui ne lui a jamais été accordé est refusé, donc une case à cocher qui ne vivrait que dans le navigateur serait de la décoration. Cocher tous les modules de données revient au joker *tous les modules*, qui garde les futurs modules de données couverts.

Les modules auxquels on peut accorder aujourd'hui :

| Module | Ce qu'il lit |
|---|---|
| **Historical Data** | la liste des fournisseurs du formulaire de téléchargement, et la recherche de symboles |
| **Visualization** | la recherche de symboles du graphique, ses fenêtres à la demande et son flux en direct |
| **Watchlists** | la source de cotations d'une liste, ou d'un symbole seul |
| **Journal** | les bougies derrière les onglets Données de marché et Risque ouvert |
| **Quant Tools** | les onglets Dérivés : contrats futures, chaînes d'options et volatilité implicite, depuis Interactive Brokers ou Massive |

## Limites de requêtes {#request-limits}

Une limite est un nombre d'appels sortants par **jour**, **heure** ou **minute**, suivi par connector.

- Partout ailleurs dans l'application elle est **en observation seule** : elle alimente les compteurs de [Paramètres → Débit API](/fr/config/settings#api-rate) et vous avertit, mais rien n'est bridé.
- Sur les récupérations à la demande du graphique (`/api/histviz/series`) elle **bloque** : une fois le connector à sa limite, la fenêtre revient avec les barres déjà stockées et un avis *limite de requêtes atteinte*, au lieu de consommer discrètement une offre à usage mesuré.

Laissez la limite désactivée si vous préférez que ce soit le fournisseur qui dise non.

## Où les connectors sont utilisés

- **Historical Data** : la liste des fournisseurs du formulaire de téléchargement, et la recherche de symboles.
- **Visualization** : l'onglet Data interroge d'un coup tous les connectors accordés au graphique ; le flux en direct tourne sur le connector que vous choisissez dans le contrôle de direct, ou sur le plus ancien accordé au graphique pour ce fournisseur.
- **Watchlists** : la source de cotations d'une liste, ou d'un symbole seul. CoinGecko et Yahoo restent disponibles sans aucun connector.
- **Trading Journal** : les bougies derrière les onglets Données de marché et Risque ouvert, avec une source au choix par type d'actif.
- **Quant Tools** : les onglets Dérivés listent les contrats futures d'un produit ou la chaîne d'options d'un sous-jacent et cotent chacun. Seuls Interactive Brokers et Massive les listent ; l'historique de volatilité implicite vient uniquement d'Interactive Brokers.

::: tip Mise à niveau depuis les anciens réglages par module
L'onglet *Paramètres* de Historical Data et l'onglet *Sources* de Watchlists n'existent plus : c'étaient deux copies déconnectées de cet écran sur deux listes déconnectées. Les comptes créés dans l'un ou l'autre sont maintenant des connectors ici, chacun toujours accordé au module d'où il vient, donc rien ne change de portée à la mise à niveau. Les noms sont de nouveau globalement uniques : un nom qui existait dans les deux listes est conservé une fois et l'autre renommé `<name> #2`.
:::
