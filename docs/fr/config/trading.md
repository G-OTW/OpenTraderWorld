# Trader depuis le graphique

Un compte broker peut faire plus que lire votre book : avec le **trading** activé, le graphique lui envoie des ordres, les déplace et les annule depuis leur ligne, et clôture les positions depuis la leur. Ces brokers acceptent des ordres aujourd'hui :

- **Interactive Brokers (Gateway)**, via l'IB Gateway ou la TWS que vous faites tourner vous-même. Cette page.
- **Binance** et **Binance USDⓈ-M Futures**, sur les comptes réels et sur leurs environnements de test (Spot Demo Mode, le testnet des futures). Un compte réel accepte des ordres une fois le [trading réel](#live-trading) activé dessus.
- **Bybit** (book spot ou linear), sur les comptes réels et sur le testnet Bybit. Classé **E2D** : construit d'après la documentation de Bybit et jamais testé contre Bybit ; un compte réel accepte des ordres une fois le [trading réel](#live-trading) activé dessus avec l'acceptation.
- **Bybit EU** (spot), sur les comptes réels et sur le testnet Bybit EU. Classé **E2D** comme Bybit, avec la même acceptation pour un compte réel.
- **OKX** (spot, swaps perpétuels et futures à échéance), sur les comptes réels et en trading démo OKX. Classé **E2D**, avec la même acceptation pour un compte réel.
- **Bitget** (spot et futures USDT-M), sur les comptes réels et en trading démo Bitget. Classé **E2D**.
- **Gate.io** (spot et perpétuels USDT), sur les comptes réels et sur le testnet Gate.io. Classé **E2D**.
- **KuCoin** (spot) et **KuCoin Futures**, sur les comptes réels seulement : KuCoin n'a pas d'environnement de test. Classé **E2D**.
- **Kraken** (spot), sur les comptes réels seulement : Kraken n'a pas d'environnement de test spot. Classé **E2D**.
- **Kraken Futures** (contrats multi-collatéral, PF_ et FF_), sur les comptes réels et sur la démo Kraken Futures. Classé **E2D**, avec la même acceptation pour un compte réel.
- **Coinbase Advanced Trade** (spot), sur les comptes réels seulement : Coinbase n'a pas de compte de test. Classé **E2D**.
- **MEXC** (spot et perpétuels USDT), sur les comptes réels seulement : MEXC n'a pas d'environnement de test. Classé **E2D**.
- **Bitfinex** (spot du portefeuille exchange et perpétuels), sur les comptes réels seulement : Bitfinex n'a pas d'hôte de test. Classé **E2D**.
- **Deribit** (futures et perpétuels : contrats linéaires USDC et contrats inverses BTC et ETH), sur les comptes réels et sur test.deribit.com. Classé **E2D**, avec la même acceptation pour un compte réel.
- **Paper trading**, un compte simulé géré par l'application elle-même. Voir [Paper trading](/fr/config/paper-trading).

Tout autre broker répond qu'il ne prend pas d'ordres depuis OpenTraderWorld, et reste en [lecture seule](/fr/config/brokers).

::: danger Expérimental
Le trading depuis le graphique est **expérimental** : il fonctionne avec Interactive Brokers, Binance, Binance USDⓈ-M Futures, Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex et Deribit, ainsi qu'avec le paper trading. Interactive Brokers et les deux comptes Binance sont longuement testés, sur des marchés simulés et sur les environnements de test des brokers. Les autres sont construits à partir de la documentation de chaque broker et n'ont pas encore été essayés sur un vrai compte. Des bugs peuvent subsister chez chacun d'eux. Vérifiez chaque ordre chez votre broker, commencez petit, et tradez à vos propres risques.
:::

::: tip Comptes réels
Interactive Brokers, Binance et Binance USDⓈ-M Futures sont classés **E4** : un compte réel accepte des ordres une fois le [trading réel](#live-trading) activé dessus, avec ses limites réglées. Les comptes paper et les environnements de test n'en ont pas besoin.

Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex et Deribit sont classés **E2D** : un compte testnet ou démo accepte des ordres d'emblée ; un compte réel demande le trading réel activé avec l'acceptation que le broker n'a jamais été testé contre la plateforme (voir [Trading réel](#live-trading)).

L'annulation d'ordres n'est jamais bloquée, quel que soit le compte.
:::

## Mise en place

### 1. La Gateway

Lancez IB Gateway (ou la TWS) connectée à votre compte paper ou réel et, dans ses réglages API :

- cochez **Enable ActiveX and Socket Clients** ;
- décochez **Read-Only API**, sinon IB refuse lui-même chaque ordre ;
- avec Docker Engine (Linux), décochez **Allow connections from localhost only** et ajoutez `172.28.53.10` aux **Trusted IPs**.

Le client id est choisi par l'application (3100 à 3115), plusieurs sessions peuvent donc partager une même Gateway.

### 2. Le compte

Dans **Paramètres → Brokers**, **Ajouter un compte**, choisissez **Interactive Brokers (Gateway)** et renseignez :

| Réglage | Valeur |
|---|---|
| **Gateway host** | `host.docker.internal` quand l'application tourne dans Docker, `127.0.0.1` sinon |
| **Gateway port** | `4002` Gateway paper, `4001` Gateway réelle, `7497` TWS paper, `7496` TWS réelle |

Aucun secret à fournir : la Gateway est déjà connectée. **Tester la connexion** vérifie qu'elle répond.

### 3. L'interrupteur de trading

Chaque ligne de compte a une puce **Trading**, à côté de ses puces de modules. Elle est séparée volontairement :

- **Tous les modules** ne l'inclut pas. Lire votre book et envoyer des ordres sont deux permissions différentes.
- **L'activer demande votre mot de passe, à chaque fois.** La désactiver ne demande rien.
- Les comptes de paper trading font exception : pas de mot de passe, aucun argent réel n'est en jeu.

### 4. Le graphique

Un graphique peut trader quand trois conditions sont réunies :

1. un compte est choisi dans le **Broker book** du graphique ;
2. le trading est activé pour ce compte ;
3. les données du graphique viennent du même broker : un compte IBKR trade un graphique alimenté par le connecteur Interactive Brokers.

Quand l'une manque, le panneau d'ordres dit laquelle et comment y remédier. Les futures demandent le **mois du contrat** : un future continu est refusé, ouvrez donc `MNQZ6` plutôt que le graphique continu.

## Le verrou de trading {#trading-lock}

Un second facteur optionnel pour les ordres. Verrou activé, le premier ordre après connexion demande le code de votre authentificateur, une seule fois, puis tous les comptes de trading restent ouverts dans ce navigateur jusqu'à la déconnexion.

- Il protège le passage, la modification et la clôture. Annuler, un ordre ou tous, ne demande jamais rien.
- Il couvre tous les comptes de trading sauf les [comptes paper](/fr/config/paper-trading).
- Activez-le dans **Paramètres → Sécurité → Verrou de trading**, une fois l'authentification à deux facteurs configurée. Détails, codes erronés et réinitialisation : [Sécurité du compte](/fr/config/security#trading-lock).

::: danger Retirer le second facteur, verrou activé, coupe le trading partout
Retirer l'authentification à deux facteurs, depuis les Paramètres ou avec `disable-totp`, désactive le verrou. Si le verrou était activé, cela coupe aussi le trading sur **tous** les comptes, paper compris. Réactivez-le ensuite compte par compte.
:::

## Envoyer des ordres

### Le ticket

L'onglet **Ticket** du panneau d'ordres envoie un ordre, un bracket ou un OCO. **Il propose ce que le broker du compte accepte, et rien de plus** : un type, une durée de validité ou une option que le broker ne déclare pas n'est pas affiché (voir [Ce que prend chaque broker](#what-each-broker-takes)).

- **Types :**
  - **Market**, **Limit**, **Stop**, **Stop limit**.
  - **Trailing stop** : un stop qui suit le marché à une distance, en pourcentage ou en montant de prix, et ne recule jamais. **Start at**, là où le broker l'a, le fait attendre un prix avant de commencer à suivre.
  - **MIT** (market if touched) et **LIT** (limit if touched) : le miroir d'un stop, déclenché quand le marché arrive au prix par le côté du limit (un achat sous le marché, une vente au-dessus), puis un ordre au marché ou limit.
  - **OCO** : un limit et un stop du même côté, le premier exécuté annulant l'autre. Ses deux prix doivent différer.
- **Sorties :** les interrupteurs **Take profit** et **Stop loss** ajoutent l'ordre inverse pour la même quantité. En activer un le préremplit à 0,5 % (take profit) ou 0,25 % (stop loss) de l'entrée. **Exits time in force** choisit GTC (par défaut) ou Day. Là où le broker accepte les ordres reduce-only, les sorties partent en reduce-only. Indisponible sur un OCO, et chez un broker qui ne rattache pas de sorties. Chez Interactive Brokers, l'entrée est retenue jusqu'à ce que ses sorties soient acceptées, le bracket part donc entier.
- **Durée de validité :** Day, GTC, GTD avec une date et une heure futures, IOC, FOK (tout ou rien, tout de suite).
- **Prix :** saisis comme un prix, ou comme une distance au bid, à l'ask ou au dernier cours (à l'entrée, pour les sorties), en ticks quand le tick est connu.
- **Taille :** en **unités** (actions, contrats), en **montant** d'argent (divisé par la valeur de l'entrée), ou en argent **risqué** jusqu'au stop loss (divisé par la distance de l'entrée au stop loss, qui doit être activé). Un future compte la valeur de son point ; contrats et actions sont arrondis à l'unité inférieure.
- **Options**, chacune affichée seulement là où le broker l'accepte :
  - **Reduce only** : ne fait que réduire la position, n'en ouvre ni n'en retourne jamais.
  - **Post only** : un limit qui ne fait que se poser dans le carnet ; refusé plutôt qu'exécuté aussitôt.
  - **Iceberg** : la taille montrée dans le carnet, le reste caché.
  - **Trigger on** : le prix que surveille un stop ou un déclencheur (dernier, mark, bid/ask, mid).
  - **Close position** : un stop ou MIT qui clôture toute la position à son déclenchement, quelle que soit sa taille.
  - **Outside RTH** : se déclenche et s'exécute aussi hors des heures de séance régulières.
  - **Adaptive algo** : le broker travaille un ordre au marché ou limit pour un meilleur prix, patient, normal ou urgent.

Avant tout envoi, l'application vérifie :

- que chaque prix et quantité est supérieur à zéro, et que chaque option convient au type (post-only sur un limit, un iceberg sous la taille de l'ordre, un pourcentage de trailing sous 100) ;
- que le broker accepte le type, la durée de validité et chaque option demandés : ce qu'il n'accepte pas est refusé, en le nommant ;
- qu'un prix tombe sur le tick de l'instrument, sinon il est refusé avec les deux prix valides les plus proches (`… is not on MNQZ6's 0.25 tick: use … or …`) ;
- que l'instrument se résout chez le broker. **Il n'est jamais deviné.**

Si une jambe d'un ticket est refusée, tout le ticket est annulé.

### Les ordres que l'application suit {#followed-orders}

Chaque ordre est stocké sous son propre id **avant** d'être envoyé, et la réponse du broker est stockée avec lui. Ainsi :

- **Un id d'ordre n'est envoyé qu'une fois par compte, à jamais.** Le graphique donne un nouvel id à chaque ordre ; un ticket renvoyé (après avoir indiqué une marge, par exemple) part sous de nouveaux ids. Un id déjà utilisé est refusé, même après un redémarrage.
- **Pas de réponse n'est pas un refus.** Un ordre que le broker ne confirme pas à temps (10 secondes pour la Gateway) est recherché chez le broker par son id. Quand il reste introuvable, il est marqué **inconnu** et n'est **jamais renvoyé** : les nouveaux ordres sur cet instrument attendent qu'une recherche tranche. S'il reste non réglé après 5 minutes, vous recevez une notification (canal de notifications du graphique) vous demandant de vérifier l'ordre chez le broker.
- **Un ordre que le broker n'a jamais reçu** (toujours introuvable après les recherches) attend votre décision. L'onglet Ordres affiche *le broker n'a jamais reçu cet ordre* avec **Renvoyer**, qui l'envoie sous de nouveaux ids en passant par tous les contrôles d'un nouveau ticket, et **Abandonner**, qui n'envoie rien. Vous recevez aussi une notification.
- **Un redémarrage n'oublie rien.** Les ordres envoyés juste avant l'arrêt de l'application sont recherchés à son démarrage, et les ordres que l'application lie elle-même (brackets Binance USDⓈ-M) sont repris.

L'application compare ensuite sa vue à celle du broker, quelques secondes après le démarrage puis en cycle (chaque minute tant qu'elle a des ordres en cours, toutes les 10 minutes sinon, et aussitôt qu'un flux en direct signale des événements manqués). **Le broker a raison** : un ordre exécuté ou annulé pendant que personne ne regardait prend l'état du broker, et un ordre passé ailleurs (TWS, le site de la plateforme) est noté comme externe. Une position qui a changé sans exécutions pour l'expliquer est une **dérive** : elle est écrite dans le journal d'audit et notifiée.

### Ce que prend chaque broker {#what-each-broker-takes}

| | Interactive Brokers | Binance spot | Binance USDⓈ-M | Paper | Bybit | Bybit EU | OKX | Bitget | Gate.io | KuCoin | Kraken | Kraken Futures | Coinbase | MEXC | Bitfinex | Deribit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **Types** | tous | tous | tous | tous | spot : market, limit ; linear : market, limit, stop, stop limit | market, limit | marché, limite | spot : marché, limite, stop ; futures : marché, limite, stop, stop limite | marché, limite | spot : marché, limite ; futures : marché, limite, stop, stop limite | marché, limite, stop, stop limite, MIT, LIT | marché, limite, stop, stop limite, MIT, LIT | marché, limite, stop limite, LIT | marché, limite | marché, limite, stop, stop limite | marché, limite, stop, stop limite, MIT, LIT |
| **Durée de validité** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK (un ordre market part IOC : FOK refusé) | GTC, IOC, FOK (un ordre market part IOC : FOK refusé) | GTC, IOC, FOK (FOK refusé sur un ordre au marché) | GTC, IOC, FOK (FOK refusé sur un ordre au marché ; un stop est GTC) | GTC, IOC, FOK (un ordre au marché travaille en IOC ; futures : FOK refusé sur un ordre au marché) | GTC, IOC, FOK (FOK sur une limite seulement) | GTC, GTD, IOC, FOK (FOK sur une limite seulement ; IOC sur un ordre limite ou au marché) | GTC, IOC, FOK (un ordre au marché travaille en IOC : FOK refusé ; stops, MIT et LIT GTC) | GTC, GTD, IOC, FOK (un ordre au marché travaille en IOC : FOK et GTD refusés ; un stop limite est GTC ou GTD) | GTC, IOC, FOK (FOK sur une limite seulement) | GTC, GTD, IOC, FOK (IOC et FOK sur une limite seulement ; un stop est GTC ou GTD) | GTC, Day, IOC, FOK (Day refusé sur un ordre au marché) |
| **Sorties, OCO** | liés par IB | listes d'ordres (OTO, OTOCO, OCO) | liés par l'application | liés par l'application | linear : take profit et stop loss sur toute la position, au marché | | | futures : take profit et stop loss d'une position, déplacés ou retirés depuis le panneau des ordres | | | |  | |  | |  |
| **Trailing stop** | montant ou %, depuis le marché | %, par pas de 0,01 %, prix de départ | % (0,1 à 20), prix de départ | montant ou %, prix de départ | | | | | | | |  | |  | |  |
| **Reduce only** | | | oui | oui | linear | | swaps et futures | futures | futures | futures | | oui | | futures | perpétuels | oui |
| **Post only** | | oui (limit maker) | oui (GTX) | oui | | | oui | oui | | oui | oui | oui | oui | oui | oui | oui |
| **Iceberg** | oui, en unités entières | oui, limits GTC | | | | | | | | | |  | |  | |  |
| **Trigger on** | dernier, bid/ask, mid | | dernier, mark | | dernier (stops linear) | | | spot : dernier ; futures : dernier, mark | | dernier (stops futures) | dernier | dernier, mark, index | dernier | |  | dernier, mark, index (indiqué sur chaque stop) |
| **Close position** | | | oui | | | | | | | | |  | |  | |  |
| **Outside RTH** | oui | | | | | | | | | | |  | |  | |  |
| **Adaptive algo** | oui | | | | | | | | | | |  | |  | |  |

- **Binance spot** retient les sorties d'une entrée **limit** (OTO, OTOCO) et place celles d'une entrée **market** une fois celle-ci exécutée ; une entrée stop n'accepte pas de sorties. La moitié limit d'un OCO part en limit maker. Une sortie de vente porte sur la quantité achetée : des frais prélevés dans la devise elle-même en laissent moins, et Binance refuse la sortie.
- **Binance USDⓈ-M** ne rattache rien lui-même : l'application envoie les sorties en reduce-only à côté de leur entrée, et les lie, ainsi que les paires OCO, en les relisant toutes les 5 secondes. Après un redémarrage, l'application reprend les liens à partir des ordres qu'elle a stockés. Le mode hedge est suivi : chaque ordre nomme son côté long ou short, et un symbole détenu dans les deux sens se clôture un côté à la fois.
- Les trailing stops **Interactive Brokers** suivent dès leur placement (pas de prix de départ), et montrent un iceberg en unités entières.

### Bybit

- **Chaque ordre va au book de son instrument.** Un compte Bybit montre ses ordres et positions de chaque book qu'il lit. Un ordre va au book de l'instrument du graphique : celui sur lequel le connecteur de données Bybit du graphique est réglé (son **Market** : spot ou linear), ou sinon le seul book qui liste le symbole. Un symbole que les deux books listent (`BTCUSDT`, paire spot et perpétuel) sur un graphique dont les données viennent d'ailleurs est refusé avec ce correctif. Le ticket propose ce que ce book accepte.
- **Testnet.** Réglez le **Site** du compte sur `testnet` (une clé de testnet.bybit.com) : ses ordres n'engagent pas d'argent et ne demandent pas le trading réel.
- **Contrôles avant l'envoi.** Tick, pas de taille, taille minimale et maximale (limit et market à part) et valeur minimale d'ordre, d'après la liste d'instruments de Bybit. Un ordre refusé nomme la règle et les valeurs les plus proches ; rien n'est modifié. Un instrument qui ne se traite pas en ce moment (pré-lancement, en livraison, fermé) n'accepte aucun nouvel ordre ; les annulations partent toujours.
- **Mode de position.** Le mode one-way ou hedge est lu sur votre compte Bybit (une fois, puis tenu à jour par le flux de Bybit) et jamais modifié. Chaque ordre linear nomme la position sur laquelle il agit.
- **Stops de protection.** Un stop linear reduce-only part avec le *close on trigger* de Bybit : il réduit la position même quand la marge manque, et Bybit peut alors annuler ou réduire d'autres ordres sur ce contrat.
- **Avant le premier ordre réel d'une session** (linear), l'ordre passe par le pré-contrôle de Bybit, qui répond la marge qu'il prendrait ; un refus à ce stade n'envoie rien. Un premier ordre conditionnel est refusé : envoyez d'abord un ordre market ou limit simple.
- **Mises à jour en direct.** Les ordres suivent le flux privé de Bybit ; les positions sont relues quand elles changent. Si le flux n'est pas disponible (une clé RSA, un site régional sans flux), le compte est lu par cycle.
- **Si l'application s'arrête**, Bybit garde vos ordres actifs : rien ne les annule.
- **Take profit et stop loss (linear).** Le take profit et le stop loss d'un ticket partent avec l'entrée en un seul appel et couvrent toute la position (mode *Full* de Bybit) : chacun part en ordre au marché quand son prix se traite. Bybit les crée quand l'entrée s'exécute ; ils apparaissent dans le panneau d'ordres comme ordres take profit et stop loss liés à l'entrée tant qu'elle travaille. Déplacer ou retirer l'un le modifie sur la position. Un take profit limite part au marché, pas en limite.
- **Pas encore proposé :** take profit et stop loss sur le spot ou sur une partie de la position, OCO, modification d'un ordre en place, trailing stops, post only.

### Bybit EU

- **Spot seulement**, par la même chaîne d'ordres que Bybit spot : ordres market et limit, les mêmes contrôles avant l'envoi, un ordre market dimensionné dans la monnaie de base.
- **Testnet.** Réglez le **Site** du compte sur `eu-testnet` (une clé de testnet.bybit.eu) : ses ordres n'engagent pas d'argent et ne demandent pas le trading réel.
- **Mises à jour en direct.** Bybit ne documente aucun flux privé pour Bybit EU : le compte est lu par cycles.
- **Votre clé.** Le guide de Bybit indique que l'API du site EU sert les applications tierces des utilisateurs API broker. Une clé personnelle peut alors être refusée : l'ordre affiche le refus de Bybit avec son code, et rien d'autre n'est envoyé.

### OKX

- **Books.** Spot, swaps perpétuels et futures à échéance. Chaque ordre va au book de son instrument : `BTC-USDT` est du spot, `BTC-USDT-SWAP` un swap, `BTC-USDT-251226` un future à échéance.
- **Région.** Réglez la **Région** du compte sur celle où il a été ouvert : `global` (okx.com, par défaut), `eea` (my.okx.com) ou `us` (app.okx.com, États-Unis et Australie). Une clé ne marche que dans sa région.
- **Trading démo.** Réglez **Environnement** sur `demo` avec une clé de trading démo : ses ordres n'engagent pas d'argent et ne demandent pas le trading réel.
- **Tailles.** Swaps et futures se traitent en contrats : une taille qui n'est pas un nombre entier de contrats est refusée, avec les tailles les plus proches.
- **Modifier un ordre** en place : son prix et sa taille.
- **Le mode de position** (net ou long/short) est lu sur votre compte et jamais changé. Le levier est affiché, jamais réglé. Les ordres spot n'utilisent que vos propres coins, jamais de marge.
- **Pas proposé :** ordres stop et à déclenchement, take profit et stop loss, stops suiveurs, OCO, iceberg, options, contrats inverses.

### Bitget

- **Books.** Spot et futures USDT-M. Un ordre va au book réglé sur le connecteur de données Bitget du graphique (son **Marché** : `spot`, ou `usdt-futures`, aussi écrit `futures`), sinon au seul book qui liste le symbole. Un symbole listé par les deux books, sur un graphique dont les données viennent d'ailleurs, est refusé avec ce correctif.
- **Trading démo.** Réglez **Trading démo** sur `yes` avec une clé démo Bitget : ses ordres n'engagent pas d'argent et ne demandent pas le trading réel.
- **Les stops** partent en ordres à déclenchement Bitget, sur le dernier prix (futures : dernier ou mark). Un stop spot s'exécute au marché.
- **Un achat spot au marché** est un montant à dépenser en devise de cotation : l'application lit le meilleur ask juste avant l'envoi et envoie ce montant, la taille exécutée peut donc différer un peu. La réponse le dit.
- **Tous les ordres du compte** s'affichent dans le panneau des ordres, ordres à déclenchement compris, et s'annulent depuis lui.
- **Take profit et stop loss d'une position (futures).** Celui posé sur une position s'affiche dans le panneau des ordres ; le déplacer ou le retirer là le change sur la position.
- **Le mode de position et la marge** sont lus sur votre compte et jamais changés. Le levier est affiché, jamais réglé.
- **Pas proposé :** modifier un ordre en place, un take profit et stop loss envoyés avec l'entrée, stops suiveurs, stop limite spot, OCO.

### Gate.io

- **Books.** Spot et perpétuels USDT, choisis comme pour Bitget : le **Marché** du connecteur de données du graphique (`spot`, ou `futures`, aussi écrit `usdt-futures`), sinon le seul book qui liste le symbole.
- **Testnet.** Réglez le **Site** du compte sur `testnet` avec une clé testnet : ses ordres et ses lectures vont au testnet Gate.io, n'engagent pas d'argent et ne demandent pas le trading réel.
- **Un achat spot au marché** est un montant à dépenser en devise de cotation, valorisé au meilleur ask lu juste avant l'envoi et arrondi vers le bas à la précision de la paire. La taille exécutée peut différer un peu ; la réponse le dit.
- **Les tailles futures** sont des contrats entiers : une taille qui n'en est pas est refusée, avec les tailles les plus proches.
- **Modifier un ordre** en place : en spot son prix ou sa taille (l'un à la fois), en futures son prix et sa taille.
- **Pas proposé :** stops, take profit et stop loss, stops suiveurs, post only, OCO, iceberg.

### KuCoin et KuCoin Futures

- **Pas d'environnement de test.** KuCoin n'en a pas : tout compte est réel et demande le trading réel activé avec l'acceptation. Avant le premier ordre d'une session, l'application l'envoie au test d'ordre de KuCoin, qui le vérifie sans le placer.
- **Spot :** ordres au marché et à cours limité, en taille de la devise de base.
- **Futures :** marché, limite, stop et stop limite sur le dernier prix, reduce only, post only. Les tailles sont des contrats entiers. Les stops s'affichent dans le panneau des ordres et s'annulent depuis lui.
- **Le mode de position et de marge** est lu sur votre compte et jamais changé. Le levier est affiché, jamais réglé.
- **Pas proposé :** modifier un ordre en place, un take profit et stop loss envoyés avec l'entrée, stops spot, OCO, futures marginés en coin, trading sur marge.

### Kraken

- **Pas d'environnement de test.** Kraken n'en a pas pour le spot : tout compte est réel et demande le trading réel activé avec l'acceptation. Avant le premier ordre d'une session, l'application l'envoie à l'appel validate de Kraken, qui le vérifie sans le placer.
- **Votre clé.** Donnez-lui Query Funds, Query Open Orders & Trades, Query Closed Orders & Trades, Create & Modify Orders, Cancel/Close Orders et Access WebSocket API, jamais Withdraw Funds. Donnez à l'application une clé à elle : Kraken refuse un appel dont le nonce n'est pas supérieur au dernier de la clé, donc une clé qu'un autre programme utilise aussi échoue au hasard.
- **Tailles et frais.** Les ordres sont dimensionnés dans la monnaie de base ; les frais sont prélevés dans la devise de cotation.
- **Modifier un ordre** en place : son prix, son déclenchement et sa taille. Tout annuler est un seul appel.
- **Pas proposé :** marge, un ordre de clôture attaché à l'entrée, stops suiveurs, iceberg, déclenchements sur le prix index, actions tokenisées.

### Kraken Futures

- **Contrats.** Les ordres vont sur les contrats multi-collatéral (PF_, FF_), dimensionnés en coin. Les contrats inverses (PI_, FI_) s'affichent avec leurs ordres et s'annulent depuis le panneau des ordres, mais n'acceptent aucun nouvel ordre.
- **Démo.** Réglez l'**Environnement** du compte sur `demo` avec une clé de la démo Kraken Futures : ses ordres n'engagent pas d'argent et ne demandent pas le trading réel.
- **Votre clé.** General API: Full Access pour envoyer des ordres ; une clé Read Only lit toujours. N'accordez jamais la Withdrawal API.
- **Modifier un ordre** en place : sa taille, sa limite et son déclenchement.
- **Tous les ordres du compte** s'affichent dans le panneau des ordres, ordres à déclenchement compris, et Tout annuler les couvre.
- **Le mode de position** est one-way. Le levier est affiché, jamais réglé.
- **Mises à jour en direct.** Aucun flux privé n'est utilisé : le compte est lu par cycle. Les frais d'une exécution s'affichent sans leur devise, que Kraken n'indique pas.
- **Pas proposé :** stops suiveurs, un take profit et stop loss envoyés avec l'entrée, OCO, options, ordres sur contrats inverses.

### Coinbase

- **Pas de compte de test.** Coinbase n'en a pas : tout compte est réel et demande le trading réel activé avec l'acceptation. Avant le premier ordre d'une session, l'application l'envoie à l'aperçu d'ordre de Coinbase.
- **Spot seulement**, dimensionné dans la monnaie de base. Un stop limite se déclenche sur le dernier prix.
- **Contrôles avant l'envoi.** Tick, pas de taille, taille minimale et maximale, d'après la liste de produits de Coinbase. Un produit en mode limit-only ou post-only n'accepte que ces ordres.
- **Modifier un ordre** en place : le prix et la taille d'une limite.
- **Tous les ordres du compte** s'affichent dans le panneau des ordres, ordres passés dans l'application Coinbase compris.
- **Votre clé.** Elle demande la permission Trade pour envoyer des ordres ; une clé View seule lit toujours.
- **Mises à jour en direct.** Les ordres suivent le flux utilisateur de Coinbase ; les exécutions sont lues par cycle. Une exécution que Coinbase a ajustée (annulation, correction) est laissée hors de l'import et nommée dans le journal du serveur ; les autres exécutions sont conservées.
- **Si l'application s'arrête**, Coinbase garde vos ordres en marche.
- **Pas proposé :** stop au marché, take profit et stop loss, stops suiveurs, OCO, futures et perpétuels.

### MEXC

- **Pas d'environnement de test.** MEXC n'en a pas : tout compte est réel et demande le trading réel activé avec l'acceptation. Avant le premier ordre spot d'une session, l'application l'envoie au test d'ordre de MEXC.
- **Books.** Spot et perpétuels USDT, choisis comme pour Bitget : le **Marché** du connecteur de données du graphique, sinon le seul book qui liste le symbole.
- **Un achat spot au marché** est un montant à dépenser en devise de cotation, valorisé au meilleur ask lu juste avant l'envoi et arrondi vers le bas à la précision de la paire.
- **Les tailles futures** sont des contrats entiers. Les nouveaux ordres ne partent qu'en mode hedge : sur un compte one-way ils sont refusés avant tout envoi (passez le compte en mode hedge chez MEXC), alors qu'une position se ferme toujours. Le mode de marge et le levier sont lus, jamais réglés.
- **Tous les ordres du compte** s'affichent dans le panneau des ordres, ordres plan futures, take profit et stop loss, et ordres suiveurs passés ailleurs compris, et Tout annuler les couvre.
- **Votre clé.** Trading spot et placement d'ordres futures, et chaque paire spot ajoutée aux paires de trading de la clé ; le placement d'ordres futures demande le KYC.
- **Pas proposé :** placer des stops, take profit et stop loss ou stops suiveurs, ordres chase, modifier un ordre en place, OCO, futures marginés en coin.

### Bitfinex

- **Pas d'hôte de test.** Bitfinex n'en a pas : tout compte est réel, sous-comptes paper compris, et demande le trading réel activé avec l'acceptation.
- **Books.** Les paires spot du portefeuille exchange et les perpétuels (`BTCF0:USTF0`). Un ordre va au book réglé sur le connecteur de données Bitfinex du graphique (son **Marché** : spot, ou derivatives, aussi écrit perp ou futures), sinon au seul book qui liste le symbole. Le trading sur marge n'est pas proposé.
- **Le levier** est celui de la position détenue, envoyé seulement s'il est un nombre entier de 1 à 100 ; sans position, Bitfinex applique le sien par défaut (10). L'application ne le choisit jamais.
- **Les prix** portent au plus 5 chiffres significatifs : un autre prix est refusé, avec les deux plus proches.
- **Modifier un ordre** ne déplace que ses prix : pour changer sa taille, annulez-le et placez-en un nouveau.
- **Votre clé.** Accès en écriture à Orders pour envoyer ; une clé en lecture seule lit toujours. Donnez à l'application une clé à elle (Bitfinex vérifie un nonce croissant par clé).

### Deribit

- **Books.** Futures et perpétuels : contrats linéaires USDC dimensionnés en coin, contrats inverses BTC et ETH dimensionnés en USD. Sur un contrat inverse, le ticket accepte la taille en coin ou en USD : une taille en USD est arrondie vers le bas au pas du contrat (10 USD sur BTC), une taille en coin est convertie au dernier prix puis arrondie vers le bas. Les positions et ordres sur contrats inverses s'affichent en USD. Vos limites de risque valorisent un ordre donné en USD à ce montant.
- **Environnement de test.** Réglez l'**Environnement** du compte sur `test` avec une clé de test.deribit.com : ses ordres n'engagent pas d'argent et ne demandent pas le trading réel.
- **Les stops** demandent leur déclenchement (dernier, mark ou index) : il n'y a pas de valeur par défaut.
- **Post only** est refusé plutôt que déplacé quand il s'exécuterait aussitôt.
- **Modifier un ordre** en place : son prix, son déclenchement et sa taille. Un stop déclenché est suivi jusqu'à l'ordre qu'il est devenu.
- **Tous les ordres du compte** s'affichent dans le panneau des ordres, ordres à déclenchement compris, et Tout annuler les couvre.
- **Une position par instrument.** Le levier est affiché, jamais réglé.
- **Pas proposé :** options, spot, un take profit et stop loss envoyés avec l'entrée, OCO, stops suiveurs, iceberg.

### Binance

- **Clé API.** Les ordres demandent une clé avec le trading activé (« Enable Spot & Margin Trading » pour le spot, « Enable Futures » pour USDⓈ-M) et les retraits désactivés. Sur un compte réel, avant le premier ordre d'une session, l'application lit les droits de la clé et refuse d'envoyer quand le trading est coupé, que les retraits sont activés ou que la clé n'est pas restreinte à des IP de confiance. Elle fait ensuite un essai à blanc de l'ordre sur la route de test de Binance.
- **Contrôles avant envoi.** Chaque ordre est tenu aux filtres du symbole tels que Binance les publie : tick, pas et limites de taille, taille d'un ordre au marché, valeur minimale et maximale d'un ordre, bande de prix autour du prix de référence courant (mark price sur les futures), et plafond d'ordres ouverts par symbole. Un ordre refusé nomme le filtre et les valeurs les plus proches qui passeraient. Rien n'est modifié pour vous.
- **Cadence des ordres.** Quand la limite d'ordres du compte serait dépassée, l'ordre est refusé avec le nombre de secondes à attendre. Il n'est jamais mis en file.
- **Modifier un stop de futures.** Binance ne peut pas modifier un ordre conditionnel sur place. L'application place d'abord le stop modifié puis annule l'ancien, de sorte qu'un stop est toujours actif. L'ordre reçoit un nouvel id (`….r1`).
- **Si l'application disparaît (USDⓈ-M).** Tant qu'elle a des ordres actifs sur un symbole de futures, l'application garde armé le compte à rebours d'annulation automatique de Binance (120 s, rafraîchi toutes les 30 s). Si elle cesse de le rafraîchir, Binance annule tous les ordres ouverts sur ce symbole, y compris ceux passés ailleurs.
- **Positions.** Les positions de futures montrent le levier et le mode de marge (cross ou isolated) réglés chez Binance. L'application ne fait que les lire.
- **Onglet Compte (spot).** Le cash est le solde USDT ; le disponible en est la part libre. L'equity est la valeur du portefeuille spot calculée par Binance en USDT, sur le réel seulement (Spot Demo Mode ne la publie pas).

### Interactive Brokers

- Une taille que le contrat n'accepte pas (sous son minimum, ou hors de son pas de taille, par exemple 0,5 d'un future) est refusée avant tout envoi, avec les tailles valides.
- Une annulation qui arrive après l'exécution de l'ordre n'est pas une erreur : l'ordre apparaît comme exécuté.
- Tant qu'IB signale avoir perdu ses serveurs (1100), les nouveaux ordres et les modifications sont refusés ; les annulations passent toujours.
- Sur un compte réel, le premier ordre de chaque connexion à la Gateway est d'abord envoyé à IB en what-if. Si IB le refuse, ou dit que le compte ne peut pas le porter, le ticket est refusé avec la raison donnée par IB.

Le ticket se place en bas, en haut, à gauche ou à droite du graphique, ou entre les lignes ou les colonnes d'une disposition. Tirez son bord intérieur pour le redimensionner (les flèches du clavier aussi, Maj pour de plus grands pas) ; la taille est gardée par position.

### La barre rapide

La **barre Achat/Vente rapide** (bouton de la barre d'outils) montre Vendre, une quantité et Acheter avec le bid et l'ask. **Un clic envoie un ordre au marché immédiatement, sans confirmation.** Déplacez-la où vous voulez ; elle se colle au bord le plus proche.

### Le `+` de l'échelle de prix

Survolez l'échelle de prix et cliquez sur **+** : le menu propose un achat ou une vente à ce prix, en limit ou en stop selon le côté du marché où il se trouve. L'ordre part aussitôt, en Day, ou s'ouvre dans le ticket avec **Ouvrir dans le ticket…**.

## Sur le graphique

- **Les ordres actifs** sont tracés comme des lignes. Tirez l'étiquette d'une ligne pour déplacer l'ordre (elle se cale sur le tick) ; un déplacement refusé remet la ligne en place. Son **✕** annule l'ordre et ses sorties.
- **Double-cliquez** une ligne d'ordre, ou passez par l'onglet Ordres, pour l'ouvrir dans le ticket et changer ses prix, sa quantité ou sa durée de validité. Le sens et le type ne changent pas. Les ordres passés dans la TWS s'affichent aussi et se modifient de la même façon s'il s'agit d'ordres market, limit, stop ou stop-limit.
- **Les positions** sont tracées à leur prix moyen, avec un **✕** pour clôturer au marché. La clôture envoie un ordre au marché pour la quantité que le broker détient à cet instant, puis **annule les sorties de la position** : les ordres actifs sur son instrument, du côté de la clôture, que le broker lie à une entrée ou marque reduce-only. Les autres ordres sur l'instrument restent.
- **Protéger avec des sorties** (le bouclier de l'onglet Positions) ouvre le ticket sur le graphique de la position avec un OCO de l'autre côté pour la quantité détenue : un take profit et un stop loss, l'un annulant l'autre, en reduce-only là où le broker l'a. La position doit être ouverte sur un graphique de ce compte.
- **Brackets :** survoler un ordre d'un bracket trace ses liens, en pointillé vert vers le take profit et rouge vers le stop loss.
- **Exécutions :** avec **Marqueurs d'entrée et de sortie** activé, les exécutions du compte sur l'instrument sont tracées comme des marqueurs à leur prix et sur leur barre (un achat pointe vers le haut, une vente vers le bas), sur 30 jours au plus. Désactivé par défaut.

**Réglages du graphique → Trading** fixe leur affichage :

| | Options |
|---|---|
| **Position** | Résultat sur la ligne (aucun, pourcentage, montant), Ligne (pleine largeur, courte, étiquette sur l'échelle de prix seulement), afficher le sens, afficher la quantité |
| **Ordre** | Ligne, afficher le sens, afficher la quantité |
| **Exécutions** | Marqueurs d'entrée et de sortie, couleurs du Marqueur d'achat et du Marqueur de vente |

## Le panneau d'ordres

À côté de **Ticket**, filtré par broker :

| Onglet | Affiche |
|---|---|
| **Positions** | symbole, sens, quantité, prix moyen, prix de marché, P&L, heure d'ouverture et frais d'entrée (sur les 7 derniers jours d'exécutions), avec un bouton de clôture |
| **Ordres** | type, quantité, limit, stop et statut, avec modification et annulation. Son **Tout annuler** annule les ordres listés dans ce navigateur |
| **Compte** | les chiffres que le broker publie, voir plus bas |
| **Journaux** | les 200 derniers appels envoyés au broker, depuis le journal d'audit |

### Chiffres du compte

**Equity, Cash, Résultat latent, Réalisé, Marge initiale, Marge de maintenance, Disponible, Liquidité excédentaire, Pouvoir d'achat, Valeur des positions**, dans la devise de base du compte, une ligne par compte que la Gateway atteint. Pour Interactive Brokers ce sont les chiffres d'IB (`NetLiquidation`, `TotalCashValue`, `UnrealizedPnL`, le `RealizedPnL` du jour, `InitMarginReq`, `MaintMarginReq`, `AvailableFunds`, `ExcessLiquidity`, `BuyingPower`, `GrossPositionValue`). Un chiffre que le broker ne publie pas n'est pas affiché : IB ne donne pas de total de commissions, cette ligne est donc absente.

### Mises à jour en direct

Ordres, exécutions et positions arrivent au fil des annonces du broker, par un seul flux pour tous les comptes de trading de la page : les messages propres de la Gateway pour Interactive Brokers, le user data stream de Binance pour Binance (l'API WebSocket sur le spot, la listen key du compte sur USDⓈ-M, toutes deux avec la clé déjà configurée du compte). Les positions de futures en mode hedge sont relues en entier plutôt que mises à jour sur place. Le book est aussi relu toutes les 30 secondes, et une seconde après chaque mouvement de position. Si le flux tombe, la page repasse à une lecture du book toutes les 4 secondes. Une Gateway déconnectée est reconnectée au prochain appel ; les ordres actifs restent chez IB entre-temps.

## Limites de risque et coupe-circuit {#risk-limits}

Vos propres limites, vérifiées avant qu'un ordre ne parte, sur chaque compte de trading (paper compris). Chacune est désactivée tant que vous ne la fixez pas, dans **Paramètres → Brokers → Limites de risque** du compte :

| Limite | Refuse |
|---|---|
| **Ordre le plus gros (taille)** | un ordre dépassant ce nombre d'unités |
| **Valeur maximale d'un ordre (devise du compte)** | un ordre valant plus que ce montant, dans la devise du compte (taille × prix × valeur du point d'un future) |
| **Position la plus grosse (taille)** | un ordre qui laisserait plus de ce nombre d'unités détenues sur l'instrument |
| **Valeur maximale d'une position (devise du compte)** | un ordre qui laisserait une position valant plus que ce montant, dans la devise du compte (taille détenue × prix × valeur du point d'un future) |
| **Corridor de prix (%)** | un limit ou un stop éloigné de plus de ce pourcentage du dernier prix du graphique |
| **Ordres par minute** | plus d'ordres que ce nombre sur la dernière minute |
| **Ordres ouverts en même temps** | plus d'ordres actifs que ce nombre |
| **Limite de perte par période** | tout ordre dès que le compte a perdu ce montant depuis le début de la période (heure, jour ou semaine, voir plus bas) : le compte est suspendu. Son unité est un **Montant** dans la devise du compte ou un **% du capital** au début de la période |
| **Fenêtre de doublon (secondes)** | le même ordre (sens, type, quantité, prix) renvoyé dans ces secondes, jusqu'à confirmation de votre part |

- Un dépassement est refusé avec la limite et la valeur. **L'ordre n'est jamais redimensionné pour entrer dans la limite.**
- **Réduire le risque passe toujours** : un ordre reduce-only, une clôture ou un ordre qui ne fait que réduire ce qui est détenu passe toutes les limites et toutes les suspensions.
- Modifier une limite demande votre mot de passe.

### Suspensions

Une suspension refuse les nouveaux ordres jusqu'à sa fin ; les annulations et les ordres qui réduisent une position passent toujours, et les ordres actifs ne peuvent pas être modifiés entre-temps (annulez-les).

- **Coupe-circuit** : il annule tous les ordres actifs de **tous** les comptes de trading (ordres passés ailleurs compris, chaque annulation dans le journal d'audit), puis les suspend tous. Dans **Paramètres → Brokers**, il demande une confirmation. Sur le graphique, il se trouve dans la barre d'outils à côté de **Gel**, tant qu'un compte trade, et fonctionne pareil : un clic l'arme pendant 3 secondes, un second clic le déclenche. Jamais de mot de passe.
- **Gel** (barre d'outils du graphique) : bloque toute nouvelle entrée envoyée depuis les graphiques, sur chaque compte connecté (paper compris), pendant 10 minutes, et annule les ordres d'entrée actifs. Clôturer, réduire, les stops et les objectifs continuent de fonctionner. Un clic l'arme, un second le lance ; rien ne l'arrête avant la fin.
- **Limite de perte** : un compte qui atteint sa limite de perte se suspend de lui-même (vérifié avant chaque ordre et à chaque cycle de réconciliation), et vous recevez une notification.
- **Dérive de position** : quand une position bouge sans exécutions pour l'expliquer (un ordre que l'application n'a pas vu, une exécution corrigée), le compte se suspend jusqu'à ce que vous l'**acquittiez**, et vous recevez une notification. Vérifiez d'abord le compte chez le broker ; l'acquittement demande toujours votre mot de passe, quelle que soit la politique ci-dessous.

Dans **Paramètres → Brokers → Risque et coupe-circuit**, vous choisissez comment une suspension prend fin, pour tous les comptes :

- **après** : le début de l'heure suivante, du jour suivant ou de la semaine suivante (lundi), dans le **Fuseau horaire** de la règle ; ou **seulement une fois levée**. Le fuseau est UTC tant que vous n'en enregistrez pas un, et le formulaire part du fuseau de votre navigateur ;
- **lever plus tôt avec mon mot de passe** : une suspension peut être levée avant terme depuis le même endroit ou depuis la bannière du graphique. Une suspension qui ne prend fin qu'une fois levée se lève toujours avec le mot de passe.

Enregistrer la règle demande votre mot de passe.

La période de perte suit le même choix (un jour quand les suspensions ne prennent fin qu'une fois levées). La perte se mesure depuis l'equity du compte au début de la période, telle que le broker la communique ; un dépôt ou un retrait pendant la période y compte. Un broker qui ne communique pas d'equity (Binance Spot Demo Mode) ne peut pas être soumis à une limite de perte : ses ordres sont refusés tant qu'une limite est fixée.

## Trading réel {#live-trading}

La chaîne d'ordres de chaque broker a un niveau, affiché sur le compte dans **Paramètres → Brokers** : E2 (testée sur un lieu d'exécution simulé), E3 (testée sur l'environnement de test du broker, pendant plusieurs jours), E4 (un essai à blanc en réel et un ordre réel à la plus petite taille ont réussi). En dessous de E4, un compte qui trade de l'argent réel est refusé par le connecteur lui-même.

| Niveau | Signifie | Ordres réels |
|---|---|---|
| **E2** | testé sur un lieu d'exécution simulé | refusés |
| **E2D** | E2, construit d'après la seule documentation de la plateforme, jamais testé contre elle | sur un compte dont le titulaire l'a accepté, avec ses limites fixées ; le contrôle de la plateforme passe avant le premier ordre de chaque session quand elle en a un |
| **E3** | testé sur l'environnement de test du broker, pendant plusieurs jours | refusés |
| **E4** | un essai à blanc en réel et un ordre réel à la plus petite taille ont réussi | une fois le trading réel activé, avec ses limites fixées |

**E2D (Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex, Deribit).** Activer le trading réel demande de cocher l'acceptation que la chaîne d'ordres de ce broker a été construite d'après la documentation de la plateforme et n'a jamais été testée contre elle ; elle est enregistrée avec sa date et affichée sur le compte. Les mêmes limites qu'en E4 sont exigées. Une réponse que le broker ne comprend pas, un ordre dont l'issue reste inconnue ou une position qui bouge sans exécutions sont traités comme sur tout broker : journalisés, notifiés et rapprochés avec le broker.

À E4, un compte réel n'accepte encore aucun ordre tant que vous n'avez pas activé **Trading réel** dessus :

- il faut l'interrupteur de trading activé, et la **limite de taille ou de valeur par ordre**, la **limite de taille ou de valeur par position** et la **limite de perte** du compte fixées ;
- le [verrou de trading](#trading-lock) est à votre choix. Activé, l'activation du trading réel et le premier ordre de chaque connexion demandent le code de votre authentificateur. Désactivé, le compte affiche un rappel et ses ordres partent sans code ;
- le compte affiche le niveau, si le broker annule vos ordres quand l'application s'arrête (Binance USDⓈ-M : oui ; spot et Interactive Brokers : non, les ordres restent actifs), et quelles limites sont fixées ;
- la désactivation ne demande rien ; couper le trading le désactive aussi, et il faut le réactiver.

Les environnements de test, les comptes démo et les comptes paper n'en ont jamais besoin.

## Journal d'audit et métriques d'ordres {#audit-log}

Chaque passage, modification, annulation, annulation globale et clôture est écrit dans un **journal d'audit en ajout seul** : heure, compte, broker, action, requête, réponse ou erreur du broker, et latence. La base refuse de modifier ou de supprimer une ligne. Les appels refusés avant d'atteindre le broker parce que le trading est coupé ou le verrou fermé ne sont pas journalisés ; tout autre refus l'est. Ce que la [réconciliation](#followed-orders) découvre (une exécution ou annulation manquée, un ordre externe, un ordre réglé après une réponse perdue, une dérive de position) est aussi journalisé, comme **reconcile**, et n'est pas compté dans les métriques d'ordres.

Dans **Paramètres → Brokers**, chaque compte de trading montre ses **métriques d'ordres** (appels, passés, refusés, latence p50 / p99, dernier ordre) et un bouton **Annuler tous les ordres**. Contrairement au [coupe-circuit](#suspensions), il ne couvre que ce compte et ne suspend rien :

- il demande confirmation, puis annule **tous les ordres actifs** du compte chez le broker, ordres passés hors d'OpenTraderWorld compris. Chez Interactive Brokers, ce sont tous les ordres des comptes de la Gateway, y compris ceux passés à la main dans la TWS ;
- il indique combien ont été annulés. Chez Interactive Brokers, il attend d'abord jusqu'à 10 secondes qu'il n'en reste aucun actif ;
- il fonctionne trading coupé et verrou fermé.

## Trade sur alerte {#trade-on-alert}

Une alerte de graphique peut envoyer un ordre quand elle se déclenche, sans personne pour surveiller, application fermée comprise. Configurez-le dans la section **Ordre** de l'alerte (*Envoyer un ordre à un compte de courtage quand cette alerte se déclenche*) ; l'alerte elle-même est décrite dans [Charts → Alertes](/fr/modules/market-data#alerts).

- **Le compte** doit avoir le trading activé, être accordé à Charts et porter le [trading en arrière-plan](/fr/config/brokers#background-trading), signé une fois dans **Paramètres → Brokers**. Un compte réel demande aussi le [trading réel](#live-trading) activé avec ses limites définies.
- **Au déclenchement** : **Entrer** envoie le ticket que vous décrivez (sens, type, taille en **Unités**, **Montant** ou **Risque**, prix comme distance au prix qui déclenche, take profit et stop loss) ; **Sortir** annule les ordres actifs sur l'instrument et clôture sa position au marché.
- **S'il est refusé** : ne rien faire de plus, annuler les ordres sur l'instrument, le clôturer aussi, ou tout annuler et clôturer toutes les positions. Quand la réponse du broker est perdue, seuls les ordres sur l'instrument sont annulés.
- **Aperçu** montre l'ordre au prix du graphique, la simulation du broker quand il en propose une, et ce qui est déjà armé sur l'instrument. **Armer** vient après un aperçu et demande votre code d'authentification quand le [verrou de trading](#trading-lock) est actif et que le compte n'est pas paper.
- **Il se déclenche une fois**, puis attend d'être réarmé. Chaque ordre passe tous les contrôles d'un ordre du graphique ([limites de risque](#risk-limits), suspensions, journal d'audit, avec l'alerte comme source), et le résultat est notifié sur les canaux de l'alerte.
- **Désarmement.** Désactiver le trading en arrière-plan ou le trading sur le compte, retirer le second facteur, activer le verrou de trading, ou le coupe-circuit désarme ce qui était armé ; réarmez-le en connaissance de cause.

## Limites

- Chez Interactive Brokers, les ordres vont au compte par défaut de la Gateway.
- Pas de changement de sens ou de type sur un ordre actif : annulez et envoyez-en un nouveau.
- Les ordres sont dans la devise du contrat, sans conversion.
- Les agents IA ([MCP](/fr/config/ai-agents)) et les workflows de l'Automator ne peuvent ni envoyer d'ordres, ni armer un trade sur alerte. Un [bot de trading](/fr/modules/robots) ou un [trade sur alerte](#trade-on-alert) n'envoie d'ordres qu'une fois armé, sur un compte où le [trading en arrière-plan](/fr/config/brokers#background-trading) est signé.
- Les comptes broker, et donc le trading, sont désactivés en [mode démo](/fr/guide/demo).
