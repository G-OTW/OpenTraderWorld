# Comptes broker

Un **compte broker** est une clé en lecture seule vers l'endroit où vous tradez réellement. Il répond à trois questions que personne ne devrait ressaisir à la main : qu'ai-je exécuté, que détiens-je, qu'ai-je en cours.

Gérez-les dans **Paramètres → Brokers**, ou depuis le bouton *Brokers* que place, à côté de son sélecteur de compte, chaque module qui lit votre book. Les deux affichent le même écran.

::: warning Lecture seule, par construction
Les comptes broker lisent, et ne font que lire : aucune route derrière eux ne passe, ne modifie ni n'annule d'ordre. Les identifiants demandés sont de type lecture seule, donnez donc exactement cela : quand un broker peut émettre une clé en consultation seule, le formulaire le dit. Si le routage d'ordres devait un jour arriver, ce sera une fonctionnalité à part, demandant ses propres clés et sa propre permission, et cela sera indiqué ici.
:::

À ne pas confondre avec les [data connectors](/fr/config/connectors) : ceux-là lisent les **prix**, ceux-ci lisent **votre compte**. Deux identifiants différents, deux listes différentes, deux autorisations différentes, volontairement.

## Ce qu'il vous faut pour vous connecter

| Broker | Identifiants | Permission à accorder | Lit |
|---|---|---|---|
| **Alpaca** | `api_key`, `api_secret` | Clé Trading API de l'environnement choisi (live *ou* paper, ce sont des clés distinctes) | exécutions, positions, ordres, avoirs |
| **Binance** | `api_key`, `api_secret` | *Enable Reading* uniquement. Pas de trading, pas de retraits | exécutions, ordres, avoirs |
| **Binance USDⓈ-M Futures** | `api_key`, `api_secret` | *Enable Reading* plus l'accès futures. Pas de trading, pas de retraits | exécutions, positions, ordres, soldes de marge |
| **Bitget** | `api_key`, `api_secret`, `api_passphrase` | *Read-only*. Pas de trade, pas de retrait | exécutions, positions, ordres, avoirs |
| **OKX** | `api_key`, `api_secret`, `api_passphrase` | *Read* uniquement. Pas de trade, pas de retrait | exécutions, positions, ordres, avoirs |
| **OANDA** | `api_token` + l'identifiant du compte | Un token d'accès personnel. **OANDA n'a pas de token en lecture seule** : le même peut trader | exécutions, positions, ordres, avoirs |
| **Coinbase Advanced Trade** | `api_private_key` + le nom complet de la clé | Clé CDP, **View** uniquement, créée en **Ed25519**. Pas de Trade, pas de Transfer | exécutions, ordres, avoirs |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` + l'identifiant du compte | Connexion OAuth accordant `ReadAccount`, `MarketData`, `openid`, `offline_access`. **Pas `Trade`** | exécutions, positions, ordres, avoirs |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | Votre login plus l'AppKey qu'émet StoneX. **Il n'existe aucun identifiant en lecture seule** | exécutions, positions, ordres, avoirs |
| **Capital.com** | `api_key`, `identifier`, `api_password` | Une clé API (l'authentification à deux facteurs doit être activée) et son mot de passe personnalisé. **Aucun niveau en lecture seule** | exécutions, positions, ordres, avoirs |
| **NinjaTrader** | `username`, `password`, `cid`, `sec` | Login de la plateforme plus la paire de clés API développeur. **Aucune clé en lecture seule** | exécutions, positions, ordres, avoirs |
| **Kraken** | `api_key`, `api_secret` | *Query Ledger & Trade History*, et *Query Open Orders* | exécutions, ordres, avoirs |
| **Interactive Brokers (Flex)** | `flex_token` + un identifiant de requête Flex | Token Flex Web Service : il lit des relevés, il ne peut pas trader | exécutions, positions, avoirs |

Les secrets sont en écriture seule : l'application ne connaît que *quels* noms sont renseignés. Ils peuvent être saisis, ou branchés depuis le [Coffre](/fr/config/settings#vault) pour qu'une clé serve plusieurs comptes.

### Notes par broker

- **Alpaca** : le réglage *Environnement* est `live` ou `paper`, et doit être dit plutôt que deviné, puisque les deux vivent sur des hôtes différents avec des clés différentes. Alpaca ne rapporte aucune commission sur une exécution, donc les trades importés n'ont pas de frais : correct pour les actions sans commission, en deçà de la réalité pour les crypto et les options, dont les frais arrivent comme des activités de compte séparées.
- **Binance**, spot comme futures, répond à son historique de trades **un instrument à la fois**, donc une récupération doit nommer les instruments (`BTCUSDT`, `ETHEUR`). Tout autre broker ici répond pour le compte entier.
- **Trois plateformes crypto s'arrêtent à 90 jours.** Bitget, OKX et Binance futures servent trois mois d'exécutions via l'API et pas plus ; tout ce qui est plus ancien est un téléchargement depuis leur site web. La fenêtre d'import montre la limite et refuse une période qui commence avant elle, plutôt que de renvoyer une demi-réponse silencieuse.
- **Un compte de dérivés n'est pas un compte spot.** Bitget, OKX et Binance futures peuvent être short, donc *Ce compte peut être short* est coché par défaut là. Sur un compte Bitget réglé sur le seul book spot, décochez-le : une vente spot sans rien d'ouvert est la vente d'une crypto achetée plus tôt, pas un short.
- **Un contrat se compte en contrats.** OKX rapporte les exécutions de dérivés en contrats et publie ce que vaut l'un d'eux (`ctVal × ctMult`), qui est lu dans sa liste d'instruments et porté comme valeur du point du trade. Les contrats Bitget USDT-M et Binance USDⓈ-M sont dimensionnés dans la crypto de base, donc la leur vaut un. Les contrats Coin-M (inverses) ne sont lus nulle part : ils sont dimensionnés dans la devise de cotation et leur PnL n'est pas une quantité fois un prix.
- **TradeStation est le seul en OAuth.** Il n'y a pas de clé statique : une connexion unique dans le navigateur produit un refresh token, que l'application échange contre un token d'accès de 20 minutes au fil de l'eau. Accordez `ReadAccount`, `MarketData`, `openid` et `offline_access` lors de cette connexion et laissez `Trade` de côté ; un token capable de trader serait un risque permanent pour rien. Une exécution ici est une *jambe d'ordre*, puisque TradeStation publie des ordres clôturés plutôt que des exécutions, et l'identité sur laquelle une nouvelle synchronisation dédoublonne est l'identifiant d'ordre plus la place de la jambe dans celui-ci.
- **FOREX.com se connecte, il n'utilise pas de clé.** Les identifiants sont le username et le password du compte plus l'AppKey qu'émet StoneX une fois ses conditions d'API signées, donc le même login peut trader : traitez-le comme un secret à accès complet et changez le mot de passe quand vous retirez le compte. Un trade importé de là ne porte **aucune commission**, car StoneX facture le spread ; si votre compte est facturé en commission à la place, les chiffres ici seront en deçà de la réalité. Le point d'entrée d'historique des trades ne prend ni date de fin ni curseur, donc une large période est parcourue vers l'avant par pages de 200.
- **Capital.com répond un jour à la fois.** Son journal d'activité plafonne l'écart entre deux dates à 24 heures, donc un an de trading coûte un appel par jour ; les périodes de plus de 400 jours sont refusées explicitement plutôt que laissées se heurter au limiteur de débit. Chaque instrument de la plateforme est un **CFD**, donc un CFD sur action est classé comme dérivé et non comme l'action, ce qu'un formulaire fiscal veut. Un identifiant de deal désigne une position plutôt qu'une exécution, donc l'identité sur laquelle une nouvelle synchronisation dédoublonne est le deal, son horodatage et sa direction ensemble.
- **NinjaTrader autorise deux sessions par login**, et une troisième ferme la plus ancienne : ce connector en tient une, et une application de trading connectée à côté tient l'autre. Son API de trading est la plateforme Tradovate qu'il a rachetée, ce qui explique que les erreurs disent `tradovateapi`. Il répond avec les exécutions que sa session peut voir et ne publie aucune limite de profondeur, vérifiez donc la ligne la plus ancienne que renvoie la première récupération avant de vous y fier pour une année fiscale. Une valeur du point de future est lue depuis le produit du contrat, jamais supposée à partir de la racine.
- **OANDA ne donne aucun token en lecture seule.** Le token d'accès personnel qui lit ce compte peut aussi le trader, donc le formulaire le dit : traitez-le comme un identifiant à accès complet et révoquez-le quand vous retirez le compte. Rien dans l'application ne s'en sert jamais pour écrire.
- **Interactive Brokers** n'est pas du tout une clé API : il lit un rapport enregistré via le Flex Web Service. La configuration, la règle de période et le réglage de fuseau horaire ont [leur propre section plus bas](#interactive-brokers-the-flex-web-service).
- Les limites de débit sont celles du broker, et chaque ligne montre celle qui s'applique. IBKR est le plus strict : il construit le relevé à la demande et refuse une seconde requête tant qu'une est en cours de génération.

## En connecter un

1. **Ajouter un compte**, choisissez le broker, et nommez-le : le nom est ce que montrent les sélecteurs des modules, donc *Kraken main* vaut mieux que *Kraken 2*.
2. Renseignez les identifiants, ou choisissez-les dans le Coffre. Un compte auquel il en manque un est affiché comme *incomplet* et ignoré par tous les modules jusqu'à ce que vous le renseigniez.
3. Renseignez les réglages non secrets dont le broker a besoin : un environnement (Alpaca, Binance futures, TradeStation, Capital.com, NinjaTrader), un identifiant de compte (OANDA, TradeStation, Capital.com, FOREX.com, NinjaTrader), le nom de clé Coinbase, l'identifiant et le décalage de requête IBKR, ou les books Bitget.
4. Choisissez les **modules** qu'il sert, ou *tous les modules*.
5. **Tester la connexion** joint le broker et indique ce qui a répondu : le numéro et le statut du compte, combien d'actifs portent un solde, ou quel relevé le token Flex a renvoyé. Si quelque chose ne va pas, l'erreur nomme le réglage à changer.

Les autorisations sont appliquées côté serveur : un module qui demande un compte qui ne lui a jamais été accordé est refusé.

## Ce que cela vous permet de faire

Quatre destinations, une seule forme. Quoi que vous importiez et où que cela atterrisse, un import de broker suit les deux mêmes rails qu'un import de fichier :

1. **Récupérer, puis regarder.** L'application interroge le broker, intègre la réponse dans ce que stocke le module (des positions pour le journal, un bilan pour le portefeuille, des cessions clôturées pour le formulaire fiscal) et vous la montre. Rien n'est écrit à cette étape, donc une récupération qui ne vous plaît pas ne coûte rien.
2. **Valider, et garder le fil.** Tout ce qui est écrit porte l'identifiant de cet import, et reste donc un seul objet ensuite : *Annuler* supprime exactement ce qu'il a créé et rien d'autre, *Garder, ne plus suivre* coupe le lien et laisse les lignes en place. Les deux vivent dans l'historique des imports du module.

Les doublons sont le travail du rail, pas le vôtre. Chaque ligne importée est empreintée, donc récupérer de nouveau une période qui chevauche reconnaît ce qui est déjà classé, le marque dans l'aperçu et l'écrit une seule fois.

### Importer des trades dans le journal

**Journal → Import → Récupérer depuis un broker**. Choisissez le compte, la période et le book où classer, et les exécutions reviennent intégrées en positions, prévisualisées avant toute écriture.

1. **Le compte.** Seuls ceux accordés au journal sont listés, et un compte incomplet le dit. Le dernier compte et la dernière période utilisés pour un book sont mémorisés, donc la récupération suivante demande deux clics.
2. **La période**, par date ou avec les pastilles *7 / 30 / 90 / 365 jours*. Lisez-la comme la fenêtre dans laquelle tombent les *exécutions*, pas la fenêtre dans laquelle les trades se sont clôturés : une position est intégrée à partir des exécutions dans la période, donc commencez assez tôt pour attraper l'entrée.
3. **Les instruments.** Binance répond à son historique un instrument à la fois, donc les symboles y sont requis et *Suggérer* propose ce que détient le compte. Partout ailleurs le champ est un filtre : laissez-le vide pour le compte entier.
4. **Les shorts.** Un compte spot ne peut pas être short, donc une vente sans rien d'ouvert est rapportée comme une erreur de ligne nommant la solution (élargir la période) plutôt que transformée en short fantôme. Sur un compte sur marge, cochez *Ce compte peut être short*.
5. **Aperçu**, puis import. Les compteurs sont exécutions, trades, dont clôturés et ouverts, plus ce qui est déjà dans le book et ce qui n'a pas pu être construit. Chaque ligne dit de quoi il s'agit avant que vous validiez.

- Même destination et même intégration qu'un import de fichier, moins le mappage : une API répond avec des champs typés, donc les questions que pose un CSV (quelle colonne est la date, la décimale est-elle une virgule) n'existent pas ici.
- **Relancer est sans danger.** L'identité d'une position est son exécution d'*ouverture*, donc élargir la fenêtre et récupérer de nouveau actualise ce qui s'est clôturé depuis et laisse le reste intact, au lieu de classer deux fois le même trade.
- **Une actualisation est purement mécanique.** Prix, quantités, frais et dates viennent de nouveau du broker ; vos notes, tags, stratégie et champs de modèle sont à vous et y survivent.

### Aligner un portefeuille sur ce que détient le compte

**Portfolio → Depuis un broker**. Il lit un **bilan**, pas un historique de trades : l'écart avec votre registre est montré ligne par ligne, et vous choisissez les lignes à aligner. Chacune que vous acceptez écrit l'unique opération qui met le portefeuille en accord.

- Un symbole que le portefeuille détient déjà se résout seul ; tout le reste est demandé, car "BTC" sur un exchange est une chaîne et un actif ici est une source de prix.
- **Le prix de revient n'est jamais inventé.** Interactive Brokers en publie un et il est utilisé. Les exchanges crypto publient une quantité et rien d'autre, donc ces lignes le disent et prennent par défaut le prix du jour, le seul prix que personne ne peut confondre avec une affirmation sur le passé.
- **Prendre le prix de l'exchange.** Sur une ligne sans prix de revient, un clic demande à la plateforme à quel prix l'actif s'échange en ce moment et renseigne le prix. La ligne nomme alors le marché qui a répondu (`BTCUSDT`, `XBT/USD`), et le dit quand ce marché cote dans autre chose que la devise propre de l'actif : un prix Binance est en USDT, pas en dollars. Un actif que la plateforme ne cote pas est laissé tel quel plutôt que valorisé depuis ailleurs, et vous saisissez le prix vous-même.

Alpaca, Binance (spot et futures), Bitget, Coinbase, Kraken, OKX, OANDA et TradeStation répondent à cette question. Interactive Brokers Flex non : un relevé n'est pas un flux de cotations, FOREX.com cote un marché par son identifiant numérique plutôt que par le nom que porte une ligne de portefeuille, et NinjaTrader sert les prix via un droit de données de marché séparé. Capital.com répond avec le mid des deux côtés qu'il négocie.

### Dessiner votre book sur le graphique

**Chart → Broker book**. Synchronisez un compte et ses positions et ordres en cours sont dessinés comme niveaux de prix sur le graphique de l'instrument correspondant, coût moyen pour une position, limite et stop pour un ordre. La correspondance se fait sur le ticker, ponctuation mise à part. Une position sans coût moyen n'a pas de ligne et est comptée comme telle.

### Lire une année fiscale

**Taxes → Depuis un broker**. Récupère les exécutions, les intègre en positions clôturées et totalise ce qui a été réalisé dans l'année fiscale, réparti sur les lignes capital, dérivés et crypto du formulaire. Il **n'écrit rien** : vous appliquez les chiffres au formulaire, et enregistrez le scénario vous-même.

- **La fenêtre n'est pas l'année fiscale.** Ce que vous avez vendu en mars a été acheté plus tôt, et sans cet achat il n'y a pas de prix de revient : reculez la date de début assez loin pour le couvrir. Une cession dont l'achat manque est rapportée, jamais valorisée face à rien.
- Chaque position clôturée est convertie au taux de **sa propre date de sortie**. Une qui n'a pas de taux est listée et exclue des totaux plutôt qu'ajoutée dans la mauvaise devise.

## Interactive Brokers : le service Flex Web Service {#interactive-brokers-the-flex-web-service}

IBKR est le seul broker ici qui ne se lit pas via une API de trading. Il se lit via **Flex**, le service de rapports d'Account Management : vous enregistrez une requête décrivant ce que vous voulez dans un relevé, et l'application récupère ce relevé en HTTPS avec un token.

### Pourquoi Flex et pas TWS

Le [connector de données de marché](/fr/config/connectors#interactive-brokers) parle le socket TWS à un Gateway que vous exécutez vous-même. Ce socket est le bon outil pour le présent, et le mauvais pour l'historique : il répond avec les positions ouvertes et les exécutions de la **session en cours**, donc *importer mes trades de mars* n'a aucune forme en socket. Flex sert une période, ce qui est exactement la question que pose un import.

La différence pratique :

| | Flex Web Service | Socket TWS / IB Gateway |
|---|---|---|
| **Ce que vous exécutez** | rien, c'est un appel HTTPS | Gateway ou TWS, connecté, sur la machine |
| **Historique** | la période de la requête, jusqu'à un an en arrière | la session en cours uniquement |
| **Identifiant** | un token qui lit des rapports | votre session live, capable de trader |
| **Utilisé ici pour** | import du journal, avoirs du portefeuille, année fiscale, positions du graphique | prix, graphiques, barres en direct |

### Pourquoi c'est la voie sûre

- **Le token ne peut pas trader.** Il est émis pour le Flex Web Service et ce service sert des relevés. Il n'y a aucun point d'entrée d'ordres derrière lui à oublier de désactiver, aucune case de permission à mal cocher. Comparez avec une clé API d'exchange, où la lecture seule est une case dont il faut se souvenir.
- **Rien ne reste à l'écoute.** Pas de Gateway en cours, pas de port API ouvert, pas de Trusted IP à déclarer, rien qui attende sur votre machine pendant que l'application est inactive.
- **Il expire de lui-même.** IBKR donne une durée de vie au token et envoie un rappel par mail avant qu'il n'expire. Un token oublié cesse de fonctionner au lieu de rester valide indéfiniment.
- **La requête est la clôture.** Un token ne peut renvoyer que ce que décrivent les requêtes que vous avez enregistrées. Limitez la requête aux trades et positions ouvertes et c'est tout ce que l'application pourra jamais voir, quoi qu'elle demande.
- Comme tout identifiant ici, le token est **en écriture seule dans l'application** : il peut être saisi ou branché depuis le [Coffre](/fr/config/settings#vault), et n'est jamais réaffiché.

### Comment se déroule réellement une récupération

1. L'application appelle `SendRequest` avec votre token et l'identifiant de requête. IBKR répond avec un code de référence et commence à **construire** le relevé.
2. Elle interroge ensuite `GetStatement` avec ce code jusqu'à l'arrivée du XML, ce qui prend normalement quelques secondes et peut être plus long pour une requête large. Un relevé encore en génération est la réponse attendue aux premiers essais, pas une erreur.
3. Le relevé est analysé en exécutions (lignes `Trade` au niveau exécution) et en avoirs (`Open Positions`), et l'application filtre ceux-ci sur la période que vous avez choisie.

Si IBKR génère encore après une minute, l'application le dit plutôt que de rester bloquée : réduisez la plage de dates de la requête, ou réessayez dans un instant.

### Le configurer

1. **Le token** : Account Management → Settings → **Flex Web Service**. Générez-en un, copiez-le une fois, notez la date d'expiration.
2. **La requête** : Account Management → Performance & Reports → **Flex Queries** → nouvelle requête *Activity*. Incluez :
   - **Trades**, niveau de détail **Execution**, pour le journal et le formulaire fiscal ;
   - **Open Positions**, pour l'alignement du portefeuille et la superposition sur le graphique.

   Enregistrez-la et notez l'**identifiant de requête**, le numéro affiché à côté de son nom.
3. Dans OpenTraderWorld : ajoutez le compte, collez le token, renseignez **Flex query id** et **Statement time offset**, puis **Tester la connexion**. Il indique le numéro du compte, la période que couvre le relevé et combien de lignes de trades il porte, ce qui est le moyen le plus rapide de voir qu'il manque une section à la requête.

::: tip Deux réglages qui décident si l'import est correct
**La période est celle de la requête, pas la vôtre.** Une requête Flex porte sa propre plage de dates (*Last 365 Calendar Days*, *Year to Date*, une fenêtre personnalisée) et le service web ne prend aucune date. Les dates que vous choisissez dans l'application **filtrent** ce que le relevé a renvoyé, donc une requête réglée sur *Last 30 days* ne produira jamais mars quelle que soit la profondeur demandée, et IBKR ne sert pas plus d'un an. Réglez la requête large, filtrez dans l'application.

**Un relevé Flex ne nomme jamais son fuseau horaire.** Il horodate dans le fuseau propre à la requête sans dire lequel, donc réglez *Statement time offset* sur ce fuseau en minutes (`-300` New York en hiver, `60` Paris) sinon chaque exécution tombe à la mauvaise heure, et les trades intraday au mauvais jour.
:::

### Ce que Flex ne fait pas

- **Pas d'ordres en cours**, donc la superposition sur le graphique dessine les positions IBKR à leur coût moyen et aucun niveau d'ordre.
- **Pas de cotations.** Un relevé n'est pas un flux de prix : le bouton *prendre le prix de l'exchange* du portefeuille est proposé par les brokers à API, pas ici. IBKR est le seul des cinq à publier un **prix de revient**, le nombre qui compte pour un registre.
- **Un relevé à la fois.** IBKR le construit à la demande et refuse une seconde requête tant qu'un est en génération, donc des récupérations consécutives sur la même requête s'attendent l'une l'autre.

## Autorisations des modules

| Module | Ce qu'il lit |
|---|---|
| **Trading Journal** | les exécutions de la période, pour l'import |
| **Portfolios** | ce que détient le compte |
| **Visualization** | positions et ordres en cours, pour la superposition sur le graphique |
| **Tax Calculator** | les exécutions d'une année fiscale |

Cocher tous les modules revient au joker *tous les modules*, qui couvre aussi les modules ajoutés dans les versions futures.

## Limites

- **Un ticker n'est jamais deviné.** Un symbole que l'application ne peut pas résoudre est une erreur nommant la solution, pas une correspondance au mieux.
- Ce qu'un broker ne publie pas reste vide plutôt que plausible : pas de prix de revient inventé, pas de frais inventés, pas de sens inventé.
- Supprimer un compte supprime ses identifiants. Ce qu'il avait déjà importé reste.
- Les comptes broker sont désactivés en [mode démo](/fr/guide/demo).
