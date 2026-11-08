# Trader depuis le graphique

Un compte broker peut faire plus que lire votre book : avec le **trading** activé, le graphique lui envoie des ordres, les déplace et les annule depuis leur ligne, et clôture les positions depuis la leur. Trois brokers acceptent des ordres aujourd'hui :

- **Interactive Brokers (Gateway)**, via l'IB Gateway ou la TWS que vous faites tourner vous-même. Cette page.
- **Binance** et **Binance USDⓈ-M Futures**, sur les comptes réels et sur leurs environnements de test (Spot Demo Mode, le testnet des futures). Un compte réel accepte des ordres une fois le [trading réel](#live-trading) activé dessus.
- **Paper trading**, un compte simulé géré par l'application elle-même. Voir [Paper trading](/fr/config/paper-trading).

Tout autre broker répond qu'il ne prend pas d'ordres depuis OpenTraderWorld, et reste en [lecture seule](/fr/config/brokers).

::: danger Expérimental
Le trading depuis le graphique est **expérimental** : il fonctionne pour l'instant avec Interactive Brokers, Binance et Binance USDⓈ-M Futures. Il est longuement testé, sur des marchés simulés et sur les environnements de test des brokers, mais des bugs peuvent subsister. Vérifiez chaque ordre chez votre broker, commencez petit, et tradez à vos propres risques.
:::

::: tip Comptes réels
Interactive Brokers, Binance et Binance USDⓈ-M Futures sont classés **E4** : un compte réel accepte des ordres une fois le [trading réel](#live-trading) activé dessus, avec ses limites réglées. Les comptes paper et les environnements de test n'en ont pas besoin.

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

| | Interactive Brokers | Binance spot | Binance USDⓈ-M | Paper |
|---|---|---|---|---|
| **Types** | tous | tous | tous | tous |
| **Durée de validité** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK |
| **Sorties, OCO** | liés par IB | listes d'ordres (OTO, OTOCO, OCO) | liés par l'application | liés par l'application |
| **Trailing stop** | montant ou %, depuis le marché | %, par pas de 0,01 %, prix de départ | % (0,1 à 20), prix de départ | montant ou %, prix de départ |
| **Reduce only** | | | oui | oui |
| **Post only** | | oui (limit maker) | oui (GTX) | oui |
| **Iceberg** | oui, en unités entières | oui, limits GTC | | |
| **Trigger on** | dernier, bid/ask, mid | | dernier, mark | |
| **Close position** | | | oui | |
| **Outside RTH** | oui | | | |
| **Adaptive algo** | oui | | | |

- **Binance spot** retient les sorties d'une entrée **limit** (OTO, OTOCO) et place celles d'une entrée **market** une fois celle-ci exécutée ; une entrée stop n'accepte pas de sorties. La moitié limit d'un OCO part en limit maker. Une sortie de vente porte sur la quantité achetée : des frais prélevés dans la devise elle-même en laissent moins, et Binance refuse la sortie.
- **Binance USDⓈ-M** ne rattache rien lui-même : l'application envoie les sorties en reduce-only à côté de leur entrée, et les lie, ainsi que les paires OCO, en les relisant toutes les 5 secondes. Après un redémarrage, l'application reprend les liens à partir des ordres qu'elle a stockés. Le mode hedge est suivi : chaque ordre nomme son côté long ou short, et un symbole détenu dans les deux sens se clôture un côté à la fois.
- Les trailing stops **Interactive Brokers** suivent dès leur placement (pas de prix de départ), et montrent un iceberg en unités entières.

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
| **Limite de perte par période (devise du compte)** | tout ordre dès que le compte a perdu ce montant depuis le début de la période (heure, jour ou semaine, voir plus bas) : le compte est suspendu |
| **Fenêtre de doublon (secondes)** | le même ordre (sens, type, quantité, prix) renvoyé dans ces secondes, jusqu'à confirmation de votre part |

- Un dépassement est refusé avec la limite et la valeur. **L'ordre n'est jamais redimensionné pour entrer dans la limite.**
- **Réduire le risque passe toujours** : un ordre reduce-only, une clôture ou un ordre qui ne fait que réduire ce qui est détenu passe toutes les limites et toutes les suspensions.
- Modifier une limite demande votre mot de passe.

### Suspensions

Une suspension refuse les nouveaux ordres jusqu'à sa fin ; les annulations et les ordres qui réduisent une position passent toujours, et les ordres actifs ne peuvent pas être modifiés entre-temps (annulez-les).

- **Coupe-circuit** : le bouton de **Paramètres → Brokers** et du panneau d'ordres du graphique annule tous les ordres actifs de **tous** les comptes de trading (ordres passés ailleurs compris, chaque annulation dans le journal d'audit), puis les suspend tous. Il demande une confirmation, jamais un mot de passe.
- **Limite de perte** : un compte qui atteint sa limite de perte se suspend de lui-même (vérifié avant chaque ordre et à chaque cycle de réconciliation), et vous recevez une notification.
- **Dérive de position** : quand une position bouge sans exécutions pour l'expliquer (un ordre que l'application n'a pas vu, une exécution corrigée), le compte se suspend jusqu'à ce que vous l'**acquittiez**, et vous recevez une notification. Vérifiez d'abord le compte chez le broker ; l'acquittement demande toujours votre mot de passe, quelle que soit la politique ci-dessous.

Dans **Paramètres → Brokers → Risque et coupe-circuit**, vous choisissez comment une suspension prend fin, pour tous les comptes :

- **après** : le début de l'heure suivante, du jour suivant ou de la semaine suivante (lundi), dans le **Fuseau horaire** de la règle ; ou **seulement une fois levée**. Le fuseau est UTC tant que vous n'en enregistrez pas un, et le formulaire part du fuseau de votre navigateur ;
- **lever plus tôt avec mon mot de passe** : une suspension peut être levée avant terme depuis le même endroit ou depuis la bannière du graphique. Une suspension qui ne prend fin qu'une fois levée se lève toujours avec le mot de passe.

Enregistrer la règle demande votre mot de passe.

La période de perte suit le même choix (un jour quand les suspensions ne prennent fin qu'une fois levées). La perte se mesure depuis l'equity du compte au début de la période, telle que le broker la communique ; un dépôt ou un retrait pendant la période y compte. Un broker qui ne communique pas d'equity (Binance Spot Demo Mode) ne peut pas être soumis à une limite de perte : ses ordres sont refusés tant qu'une limite est fixée.

## Trading réel {#live-trading}

La chaîne d'ordres de chaque broker a un niveau, affiché sur le compte dans **Paramètres → Brokers** : E2 (testée sur un lieu d'exécution simulé), E3 (testée sur l'environnement de test du broker, pendant plusieurs jours), E4 (un essai à blanc en réel et un ordre réel à la plus petite taille ont réussi). En dessous de E4, un compte qui trade de l'argent réel est refusé par le connecteur lui-même.

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

## Limites

- Chez Interactive Brokers, les ordres vont au compte par défaut de la Gateway.
- Pas de changement de sens ou de type sur un ordre actif : annulez et envoyez-en un nouveau.
- Les ordres sont dans la devise du contrat, sans conversion.
- Les agents IA ([MCP](/fr/config/ai-agents)) et les workflows de l'Automator ne peuvent pas envoyer d'ordres.
- Les comptes broker, et donc le trading, sont désactivés en [mode démo](/fr/guide/demo).
