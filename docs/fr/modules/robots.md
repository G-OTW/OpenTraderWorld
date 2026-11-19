# Bots de trading {#trading-robots}

Un **bot de trading** est une stratégie de Backtest qui tourne en continu. Il fige les réglages de la stratégie, tourne à chaque clôture de l'unité de temps de la stratégie sur un ensemble d'instruments, et soit simule ses exécutions, soit envoie ses ordres à un compte broker. Par-dessus la stratégie, chaque bot a son propre argent (une **politique de capital**), des **exclusions**, des **garde-fous** qui réagissent à ses propres résultats, et une **priorité**.

Il n'y a pas de second moteur : chaque exécution lit la fenêtre avec le backtest ordinaire et n'agit que sur la bougie qui vient de clore. Un bot à signaux reporte les positions qu'il a prises telles qu'il les a prises ; ce que la fenêtre montre sur les bougies antérieures est de l'historique, jamais enregistré ni envoyé.

La stratégie donne les signaux, son stop et son objectif, et ses types d'ordre. Le bot donne l'argent et le calendrier : sa [politique de capital](#capital-policy) et ses [exclusions](#exclusions). L'instrument (valeur du point, pas de cotation, pas de taille, minimum) est celui de la place. Le capital, la taille, les filtres de jours et de séances et les coûts de la stratégie ne sont pas lus.

Le module exige que **Backtest** soit installé. Il se trouve à **/robots**, sous le nom **Trading Bots** dans le sélecteur de modules.

## La flotte

La page de la flotte liste tous les bots : des cartes sur un téléphone, un tableau triable sur un ordinateur. Elle se rafraîchit toutes les 10 secondes tant que la page est visible, et les comptes à rebours avancent chaque seconde.

- **Chiffres clés** : **Bots** (avec combien sont actifs), **Capital** (réparti par bot), **Aujourd'hui** (le P&L du jour, et les événements non lus), **Ouvertes** (les positions ouvertes, et combien de bots en détiennent une).
- **P&L de la flotte** : la courbe d'équité cumulée des bots, avec **P&L**, **Réalisé** et **Latent**. Filtrez-la par environnement (**Tous les environnements**, Simulé, Papier, Testnet, Réel) et par une date **Du** et **Au**. Sans date de fin, le dernier point est valorisé aux positions ouvertes.
- **Classement** : les bots triés par le P&L des trades clôturés **Aujourd'hui**, sur **1S**, **1M** ou **Tout**, avec le rendement sur leur capital de départ.
- **Exposition par type d'actif** : ce que les positions ouvertes engagent au dernier prix, réparti par type d'actif (crypto, futures, forex, actions...), avec le nombre de bots sur chacun.
- **Filtres d'état** : un clic sur une pastille montre les bots dans cet état (**Actif**, **Retenu par un garde-fou**, **Arrêté**, **Désarmé, à réarmer**, **Dégradé**, **Bloqué**, **Courtier injoignable**, **Erreur**), avec un compteur sur chacune. Un second clic l'efface. Le sélecteur de compte se restreint à **Simulé** ou à un compte.
- **À traiter** : les bots en erreur, retenus par un garde-fou ou avec des événements non lus, chacun avec la raison.
- **Prochains ticks** : les huit bots dont l'échéance est la plus proche, avec leurs instruments et un compte à rebours.
- **Marquer N comme lus** efface les événements non lus de tous les bots. **Nouveau bot** ouvre l'assistant.

## Créer un bot {#create-a-robot}

**Nouveau bot** sur la page de la flotte, ou **Lancer en bot** dans Backtest : sur un résultat (l'exécution exactement telle qu'elle a tourné) ou sur une stratégie enregistrée dans la bibliothèque. Dans les deux cas l'assistant s'ouvre, une étape à la fois :

1. **Stratégie** : le nom du bot, et une stratégie enregistrée à figer (ou l'exécution d'où il vient). **Créer une stratégie** ouvre Backtest. Une modification ultérieure de la stratégie ne change rien au bot tant que vous ne le mettez pas à jour (voir [Réglages](#settings)).
2. **Instruments** : **Ajouter un instrument** ouvre le sélecteur de données du graphique, qui interroge la recherche de symboles de chaque fournisseur. Aucun historique stocké n'est nécessaire : chaque exécution récupère les bougies que lit la stratégie. Jusqu'à 8 instruments, sur une seule **Unité de temps** qu'ils partagent tous. Chaque instrument est lu via la source de données depuis laquelle il a été choisi, que **Réglages du bot** peut changer ensuite. Là où le fournisseur diffuse un prix en direct, stops, objectifs et limites se déclenchent dessus ; ailleurs ils sont vérifiés à la clôture de la bougie. Les réglages du bot disent lequel s'applique à chaque instrument.
3. **Compte** : **Simuler** (rien n'est envoyé nulle part) ou **Compte courtier** (voir [Sur un compte broker](#on-a-broker-account)).
4. **Capital** : la [politique de capital](#capital-policy).
5. **Garde-fous** : les [garde-fous](#guards). Un nouveau bot démarre avec le jeu par défaut.
6. **Planning et notifications** : le bot tourne à chaque clôture de ses bougies et décide sur la bougie qui vient de clore. L'étape contient ses [**Exclusions**](#exclusions), un **Rapport de statut** optionnel (**Envoyer un rapport de statut**, puis quand il part : toutes les N minutes, quotidien, hebdomadaire, mensuel ou une fois, dans un fuseau horaire), **Bougies glissantes** (combien d'historique chaque exécution fournit au moteur, 5000 par défaut), **Priorité**, **Notifier** (**Chaque exécution**, **Seulement sur une nouvelle exécution**, **Jamais**) et **Canaux**.
7. **Vérification**, puis **Créer le bot**.

Le serveur revérifie tout à la création du bot. Un refus est affiché tel qu'il a été formulé, et nomme ce qu'il faut changer.

Le bot démarre aussitôt. La fenêtre **Démarrage de** *nom du bot* suit sa première exécution pendant qu'il télécharge son historique et apprend son book ; **Continuer en arrière-plan** la ferme et ouvre la page du bot pendant que l'exécution continue. Une fois prêt, un bot simulé tourne à chaque clôture à partir de là, et un bot sur broker propose **Armer** dans la même fenêtre (voir [Sur un compte broker](#on-a-broker-account)). **Ouvrir le bot** ouvre sa page.

Les canaux sont les canaux de notification accordés à Trading Bots ; le bouton de canal de l'étape les ajoute ou les gère sans quitter l'assistant. Sans canal choisi, le bot envoie à tous les canaux accordés à Trading Bots ; sans canal accordé, rien n'est envoyé et chaque événement est quand même enregistré dans le journal du bot. **Notifier** **Jamais** n'envoie rien du tout. Le rapport de statut donne l'état du bot, ses chiffres depuis le dernier rapport et ce qu'il détient ; il n'est pas envoyé tant que le bot est en exclusion et ne détient rien. **Réglages du bot** contient aussi les alertes sur les entrées et les sorties par instrument, et votre propre formulation pour les messages d'entrée et de sortie, et pour le résumé quand un rapport de statut est actif.

### Exclusions {#exclusions}

Les exclusions sont des périodes où le bot ne prend aucune nouvelle entrée. Les sorties y sont toujours gérées. Elles se lisent dans le fuseau horaire du bot, et tout mélange fonctionne :

- **Jours entiers** : les jours de la semaine choisis.
- **Plages horaires** : d'une heure à une autre, la fin exclue ; une plage qui finit avant de commencer passe minuit.
- **Périodes datées** : d'une date à une autre, les deux jours inclus.

Jusqu'à 20 plages horaires et 20 périodes datées. Avec **Sortir du marché**, le bot clôture ses positions quand une période commence, et trade de nouveau quand elle se termine. L'entrée et la sortie d'une période figurent dans le journal, et partent vers les canaux du bot quand il détient une position.

## La page d'un bot {#a-robots-page}

L'en-tête garde l'état du bot sous les yeux : son nom, la stratégie et sa version, des badges pour l'état, l'environnement d'un bot sur broker (**Papier**, **Testnet**, **Réel**), le garde-fou (en pause, arrêté, ou la réduction, comme x0.5) et **Armé** ou **Désarmé**. En dessous : la prochaine évaluation en compte à rebours, la dernière évaluation et la dernière synchro.

- **Exécuter** lance le bot tout de suite. **Arrêter la stratégie** met sa planification en pause ; cela ne ferme pas les positions et n'annule pas les ordres. **Démarrer la stratégie** la reprend.
- **Analyses** lit les trades clôturés du bot, voir [Analyses](/fr/modules/journal#insights).
- Tant qu'un garde-fou retient le bot, un avis dit pourquoi, avec **Reprendre**.

La page se rafraîchit toutes les quelques secondes ; les positions ouvertes suivent le prix en direct là où le fournisseur en diffuse un. Huit onglets :

| Onglet | Montre |
|---|---|
| **Vue d'ensemble** | **Capital**, **PnL réalisé**, **PnL latent**, **Aujourd'hui** (P&L et ordres envoyés), **Taux de réussite**, **Facteur de profit**, **Drawdown max**, **Espérance** ; la courbe d'équité ; le dernier prix de chaque instrument (en direct, bougie stockée, ou obsolète) ; **P&L par instrument** et **P&L par sens** ; le calendrier du P&L ; un graphique à l'unité de temps de la stratégie avec les entrées et les sorties. Un bot sur broker montre aussi son [panneau d'exécution](#on-a-broker-account) |
| **Positions** | instrument, sens, quantité à la taille du bot, prix moyen, prix (en direct ou stocké), latent, stop, objectif et âge |
| **En attente** | les ordres d'un bot simulé qui attendent la prochaine ouverture ; ceux d'un bot sur broker en cours chez le broker |
| **Trades** | les trades clôturés, 25 par page, avec la réduction du garde-fou là où elle s'est appliquée. **Non pris** compte les entrées que le bot a refusées et liste les 10 dernières, chacune avec sa raison |
| **Statistiques** | les statistiques des trades, la courbe de performance, et pour les stratégies à signaux un nuage 3D et les séries |
| **Garde-fous** | les règles en phrases, ce qu'elles retiennent maintenant, et la chronologie des 100 derniers déclenchements, rétablissements et reprises, du plus récent au plus ancien |
| **Journal** | les 200 derniers événements : entrées, sorties, refus, actions des garde-fous, erreurs |
| **Réglages** | la stratégie, le compte, la planification, la priorité, le capital et les garde-fous, avec **Réglages du bot**, la mise à jour vers la version de la stratégie, et **Supprimer ce bot** |

Le book d'un bot commence à sa première exécution : seul ce qui arrive à partir de la bougie qui vient de clore est enregistré. Ce que la fenêtre montre avant est de l'historique, jamais dans son book, ses statistiques ni ses garde-fous.

### Réglages {#settings}

**Réglages du bot** modifie le nom, la source de données de chaque instrument (**Source par défaut du fournisseur**, ou un connector accordé à Trading Bots), les exclusions, le rapport de statut, la fenêtre, la priorité, le capital, les garde-fous et les notifications. La stratégie, les instruments et le compte sont figés : une autre stratégie ou un autre compte, c'est un nouveau bot. La note **Suivi en avant** liste les instruments et les types d'ordre d'entrée et de sortie de la stratégie : ses signaux, son stop et son objectif tournent tels qu'ils ont été figés, tandis que son capital, sa taille, son calendrier et ses coûts ne sont pas lus.

**Passer à la version actuelle de la stratégie** (affiché quand le bot vient d'une stratégie enregistrée) reprend la stratégie enregistrée telle qu'elle se lit maintenant. Le book du bot est vidé et réappris à l'exécution suivante, et un bot armé est désarmé.

Supprimer un bot supprime son book, la chronologie de ses garde-fous et son journal.

## Politique de capital {#capital-policy}

L'argent du bot dimensionne chaque décision de la stratégie : la stratégie tourne sur le capital du bot, pas sur le sien. Chaque champ est optionnel :

| Champ | Signification |
|---|---|
| **Allocation** | le capital avec lequel le bot trade. Vide : le capital et la taille de la stratégie |
| **Devise** | la devise du compte, pour lire les chiffres |
| **Part par position (%)** | une position vaut cette part du capital du bot. Demande une allocation |
| **Levier** | le bot dimensionne avec, jusqu'à 125. Vide : 1 |
| **Entrées par position** | combien d'entrées une position peut prendre, jusqu'à 50. Vide : 1. Un bot sur broker n'en prend qu'une |
| **Positions ouvertes max** | une entrée au-delà n'est pas prise |
| **Ordres par jour max** | entrées comptées par jour UTC. Les sorties ne sont jamais retenues |

Chaque entrée est ensuite multipliée par la réduction d'un garde-fou et arrondie au pas de taille de la place (jamais vers le haut). Sur un compte broker, la **Position maximale (unités)** fixée à la création refuse une entrée plus grande au lieu de la réduire.

**Une entrée refusée nomme sa correction**, dans l'onglet Trades et le journal :

> the bot already holds 3 position(s), its maximum of 3: the entry on BTCUSDT was not taken. Raise the maximum of positions

Les autres se lisent de la même façon : une taille sous le pas de la place (augmentez l'allocation, la part par position ou le levier) ou sous son minimum (augmentez l'allocation, la part par position ou la taille de la stratégie), le maximum d'ordres du jour, une taille au-delà de la position maximale.

## Garde-fous {#guards}

Les garde-fous sont des règles sur les **propres trades clôturés** du bot, à la taille du bot. Jusqu'à 10 par bot. Les sorties sont toujours gérées, quoi que retienne un garde-fou.

Chaque garde-fou associe une condition à une action, n'importe laquelle avec n'importe laquelle.

| Quand | Alors |
|---|---|
| **pertes consécutives**, **gains consécutifs** (un nombre) | **pause** pendant des jours ou des heures (d'une minute à 365 jours) |
| **drawdown %**, **depuis le pic** ou **depuis le départ** | **arrêt jusqu'à reprise** |
| **perte % sur**, **perte en montant sur**, **pertes sur**, **gain % sur** une fenêtre : **aujourd'hui**, **cette semaine**, **ce mois**, **depuis le début** (dans le fuseau horaire du bot) | **réduire la taille** : **Diviser par** plus de 1 et au plus 10, **Retour à la taille pleine** **après trades**, **après gains**, **après jours** ou **à la main** |

Le jeu par défaut proposé à un nouveau bot : pause d'un jour après 3 pertes consécutives, et taille divisée par deux après un drawdown de 10 % depuis le pic, retour à la taille pleine après 5 trades.

- Un bot en pause ou arrêté ne prend aucune nouvelle entrée et continue de gérer ses positions. Un bot réduit dimensionne ses prochaines entrées à la baisse.
- Une pause se termine d'elle-même. Un arrêt attend **Reprendre**. Une réduction revient à la taille pleine selon sa règle de rétablissement, en une étape.
- Une règle de comptage recompte depuis son dernier déclenchement : une pause après trois pertes se redéclenche après trois de plus. Une règle de niveau se déclenche quand le niveau est franchi, et de nouveau une fois qu'il n'était plus atteint ou que sa fenêtre a tourné.
- Plusieurs à la fois : un arrêt l'emporte sur une pause, une pause l'emporte sur une réduction, et une réduction déclenchée avec eux s'applique quand les entrées reprennent. Une réduction sur une réduction se multiplie, jamais sous x0.1.
- Les pourcentages portent sur le capital de départ du bot (l'allocation, sinon le capital de départ de la stratégie). Un drawdown depuis le pic porte sur le capital au pic ; une perte ou un gain sur aujourd'hui, cette semaine ou ce mois porte sur le capital au début de cette période.
- **Reprendre** efface tout ce que retient un garde-fou. Modifier les règles conserve ce qui est retenu maintenant.

Chaque déclenchement et rétablissement figure dans l'onglet **Garde-fous** et le journal, et part vers les canaux du bot.

## Priorité et limites de débit

Chaque bot a une **Priorité** de 0 à 1000, 100 par défaut. **La plus basse passe en premier** quand plusieurs bots arrivent à échéance en même temps ou partagent un broker. Les bots dus à la même clôture tournent côte à côte, jusqu'à 8 à la fois.

Les bots sur un compte broker sont synchronisés toutes les 15 secondes, broker par broker, par ordre de priorité :

- un passage envoie au plus 10 ordres à un broker, bot par bot par ordre de priorité. Les sorties et les clôtures ne sont jamais retenues par cette limite ; une entrée au-delà de la limite attend le passage suivant, et le bot le dit ;
- quand un broker refuse des requêtes pour sa limite de débit, **seuls les bots sur ce broker attendent** : jusqu'à l'heure qu'il a donnée, ou pendant un passage quand il n'en a donné aucune. Chaque bot affiche une note qui nomme l'attente et ce qu'il faut changer (moins de bots ou d'instruments sur le compte, ou une limite plus haute sur son abonnement). Les bots sur d'autres brokers continuent.

## Sur un compte broker {#on-a-broker-account}

Un bot peut envoyer ses ordres à tout compte dont le [trading](/fr/config/trading) est activé et accordé à **Trading Bots** dans [Paramètres → Brokers](/fr/config/brokers), y compris un compte de [paper trading](/fr/config/paper-trading). Le badge d'environnement dit où ils vont : **Papier** (le broker paper de l'application), **Testnet** (le compte de test ou de démo d'une place), **Réel** (de l'argent réel).

Le moteur décide à chaque clôture et le compte exécute. À la création :

- seule une stratégie à signaux s'exécute sur un broker, avec une entrée par position (**Entrées par position** 1) ;
- chaque instrument est recherché sur le compte, qui affiche le symbole et le pas de taille trouvés. Un instrument que le compte ne sait pas nommer est refusé : le bot ne devine jamais ;
- **Sorties** : **Chez le courtier** (le stop et l'objectif reposent chez le broker, attachés à l'entrée, là où le broker peut les tenir) ou **Surveillées** (vérifiées sur le prix en direct et clôturées au marché une fois franchies) ;
- plusieurs bots peuvent viser un même instrument sur un même compte, mais un seul à la fois est armé, pour que deux bots ne tradent jamais une même position l'un contre l'autre.

Un bot sur broker est **créé désarmé** : rien n'est envoyé tant que vous ne l'**Armez** pas, dans la fenêtre **Démarrage de** *nom du bot* une fois sa première exécution terminée, ou plus tard depuis sa page. Armer demande que le **trading en arrière-plan** soit signé sur le compte (**Signer le trading en arrière-plan** ouvre les conditions à signer), et votre code d'authentification quand le [verrou de trading](/fr/config/trading#trading-lock) est actif et que le compte n'est pas paper. À partir de la clôture suivante, les entrées partent au marché et les sorties comme la stratégie le décide, sans personne pour surveiller. Chaque ordre passe les [limites de risque](/fr/config/trading#risk-limits) du compte et figure dans le [journal d'audit](/fr/config/trading#audit-log).

Armer lit d'abord le compte : c'est refusé quand le broker est injoignable, ou quand le compte détient moins que l'argent du bot (son allocation, sinon le capital de la stratégie). Chaque bot est vérifié seul, plusieurs bots peuvent donc partager un compte ; une entrée que le compte ne peut plus payer n'est pas envoyée, et le journal dit ce qu'il fallait et ce que le compte avait.

Le **panneau d'exécution** de la Vue d'ensemble montre le niveau du compte, ses limites, le book du moteur à côté de celui du compte, et le prix simulé de chaque trade à côté de celui du broker. Son état :

| État | Signifie |
|---|---|
| **Désarmée** | rien n'est envoyé tant que vous n'armez pas le bot |
| **Synchronisée** | le compte détient ce que le bot attend |
| **Dégradée** | une entrée n'a été exécutée qu'en partie. Le bot n'ajoute aucune exposition et continue de gérer ses sorties jusqu'à ce que vous soldiez ou adoptiez |
| **Arrêtée, à réarmer** | une position a été ouverte à la main sur l'instrument du bot alors qu'il était à plat, ou la position du bot a été clôturée, augmentée ou réduite à la main. Le bot a été désarmé et ses entrées en attente annulées ; les sorties d'une position clôturée le sont aussi. Réarmez-le pour continuer |
| **Envoi en pause** | un garde-fou retient les nouvelles entrées, ou le trading est suspendu sur le compte : seul ce qui réduit le risque est envoyé |
| **À vérifier chez le courtier** | un ordre n'a reçu aucune réponse : plus rien n'est envoyé sur cet instrument tant que ce n'est pas réglé |
| **Courtier injoignable** | le broker n'a pas répondu trois synchronisations de suite (45 s). Rien n'est envoyé tant qu'il ne répond pas, et le compte est relu avant tout envoi |
| **Erreur de compte** | le compte ne peut pas être utilisé (ses identifiants ou son autorisation) : rien n'est envoyé tant qu'il ne peut pas l'être |

Le bot lit le compte à chaque synchronisation (toutes les 15 s, et en premier après un redémarrage) avant d'envoyer quoi que ce soit. Toute modification à la main de la position du bot l'arrête jusqu'à ce que vous le réarmiez. Un stop ou un objectif déplacé à la main chez le courtier reste où vous l'avez mis : le bot ne le déplace plus. Annulez à la main son stop et son objectif, et la synchronisation suivante les replace, une fois ; ensuite le bot les surveille sur le prix en direct. Annuler à la main n'importe quel autre de ses ordres en attente ne change rien pour lui. Le contrôle de position du compte laisse l'instrument d'un bot au bot : les trades d'un bot ne suspendent jamais le compte.

**Positions** et **En attente** ne montrent que ce qui est chez le broker. Une entrée qui n'est pas partie (refusée par un plafond, un garde-fou, les règles de taille du venue, les fonds du compte ou le broker) est une ligne du **Journal**, sa raison exacte au survol. Les prix et le P&L d'un trade clôturé sont ce que ses ordres ont exécuté chez le broker, frais compris, une fois sa sortie exécutée ; d'ici là, le chiffre simulé tient. Un stop ou un objectif exécuté chez le broker, Arrêter et solder, ou une clôture faite par vous chez le broker clôture le trade du bot à ce prix, même quand les bougies du bot n'ont jamais atteint le niveau ; une entrée qui n'est pas partie n'est pas prise. Une bougie que le fournisseur corrige après sa clôture ne change rien : le bot fait ce qu'il a décidé à cette clôture, et un retournement fait un seul message.

- **Désarmer** arrête l'envoi. Les ordres déjà chez le broker y restent.
- **Arrêter et solder** désarme, annule les ordres en cours du bot et clôture au marché ce qu'il détient.
- **Adopter l’écart** vous laisse ce que le compte détient en dehors du bot, pour qu'un bot dégradé trade de nouveau.

Armer, désarmer, solder et adopter se font toujours par vous depuis la page, jamais par un agent ou un workflow.

## Sessions paper

Les sessions paper de Backtest sont désormais des bots, avec leur book, leurs événements, leur compte et leurs autorisations de canaux. Un ancien lien de session paper ouvre la page du bot. **Lancer en bot** remplace **Paper trade** dans Backtest.

## Bon à savoir

- Les [agents IA (MCP)](/fr/config/ai-agents) peuvent lire les bots (la liste, un bot, son tableau de bord, ses trades, les événements de ses garde-fous, son panneau d'exécution, la flotte, la boîte de réception et les variables de message). Créer, modifier, lancer ou armer un bot reste dans l'application.
- Un bot lit les bougies et le prix en direct de chaque instrument via la source de données choisie pour lui, sinon un [connecteur de données](/fr/config/connectors) accordé à Trading Bots, sinon un connecteur accordé à Backtest, sinon le premier connecteur du fournisseur.
- En [mode démo](/fr/guide/demo), le module est en lecture seule : deux bots préchargés sur un compte paper de démo, l'un réduit par son garde-fou et l'autre en pause, aucun armé ni en cours d'exécution.
- Comment un bot simulé s'exécute sur le prix en direct, et les options qu'il partage avec le backtest : [Backtest](/fr/modules/market-data#backtest).
