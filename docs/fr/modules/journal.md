# Trading Journal

Consignez chaque trade, dans n'importe quelle devise, et obtenez des statistiques de performance honnêtes : courbe d'équité, taux de réussite, espérance, profit factor, drawdown, Sharpe et plus. Le journal est organisé en dix onglets : **Breakdown**, **Analytics**, **PnL Calendar**, **Trades**, **Strategies & capital**, **Tags**, **Templates**, **Fees & currency**, **Import**, **Pending tasks**.

## Catégories

Les trades vivent dans des **catégories** : des dossiers comme *Scalping crypto* ou *Actions long terme*, chacun avec sa couleur, son capital et ses statistiques. Créez-les depuis la barre des catégories ; glissez pour réordonner. Supprimer une catégorie supprime ses trades.

## Définir le capital

Dans **Strategies & capital**, donnez à chaque catégorie un **capital de départ** et enregistrez au fil du temps les **recharges** et les **retraits**. C'est par rapport à cela que sont calculés le rendement, la courbe d'équité et le drawdown ; sans cela vous obtenez le PnL, mais pas les rendements.

Vous pouvez aussi nommer des **stratégies** avec leurs noms de signaux (p. ex. *Breakout, Pullback*). Étiquetez les trades avec une stratégie/un signal et l'onglet Breakdown peut filtrer dessus : c'est ainsi que vous découvrez quels setups paient vraiment.

## Modèles

Les modèles pilotent le formulaire de trade. Un **trade standard** prédéfini existe ; créez les vôtres par marché ou par style :

- Les **champs réservés** (sens, prix, quantité, frais, levier, multiplicateur, devise, type d'unité…) alimentent les statistiques de performance.
- Les **champs personnalisés** (texte, nombres, listes de choix…) sont libres : note du setup, condition de marché, tout ce que vous suivez.
- Un modèle peut définir un **barème de frais par défaut**, présélectionné quand on consigne depuis lui (modifiable par trade).

## Consigner des trades

Depuis l'onglet Trades, choisissez un modèle (ou le modèle *Quick*, qui montre tous les champs) et remplissez le formulaire. Deux niveaux :

- **Simple** : une entrée, une sortie (ou laissez la sortie vide pour une position ouverte).
- **Avancé** : renforcement/allègement avec plusieurs **jambes d'entrée et de sortie** (chacune avec son prix, sa quantité, ses frais, son signal), plus des **ordres SL/TP liés**. Quand un ordre lié se déclenche, cochez-le et il se replie en jambe de sortie.

Le formulaire prévisualise l'entrée moyenne, le PnL net et la quantité ouverte pendant que vous tapez. Vous pouvez joindre jusqu'à deux images (captures de graphique), choisir le levier et le multiplicateur de contrat pour les dérivés, et écrire votre propre retour sur le trade.

**Le PnL est calculé à la lecture** et gère les positions partiellement ouvertes. Le prix de revient est commutable entre **coût moyen pondéré** (par défaut) et **FIFO** (utile pour l'export fiscal), et le choix est appliqué pour de vrai, dans les statistiques comme dans l'aperçu du PnL en direct du formulaire.

## Frais

Dans **Fees & currency**, enregistrez des **barèmes de frais** : fixes ou en pourcentage, facturés par lot, unité, contrat ou trade (p. ex. *Actions IBKR : 0,05 % par trade*). Sélectionner un barème sur un trade calcule automatiquement les frais ; des frais saisis manuellement l'emportent toujours.

## Multi-devises et change

Les trades gardent la devise dans laquelle vous les avez saisis. La **devise de répartition** (affichage) est convertie avec un flux de change quotidien qui rattrape automatiquement les taux chaque jour ouvré, en reportant les taux sur les week-ends et jours fériés.

Si un taux ne peut pas être récupéré pour une date, ces trades sont **exclus des totaux convertis** et apparaissent dans **Pending tasks**, où vous saisissez à la main les taux manquants en base USD (1 USD = … de cette devise) et les trades comptent de nouveau.

## Breakdown (vos statistiques)

Par catégorie ou pour toutes, filtrable par plage de dates, ticker, sens, classe d'actifs, stratégie, signal et tag. La barre de filtres est partagée avec Analytics, donc une portée définie sur un écran est la portée de l'autre, et la liste des tickers propose les symboles que le journal contient réellement :

- **Courbe d'équité** dans la devise d'affichage.
- PnL réalisé · Rendement · Taux de réussite · Trades (clôturés/ouverts) · Espérance · Profit factor · Gain moyen / Perte moyenne · Meilleur / Pire trade · Drawdown max · Sharpe et Sortino · Frais totaux · Capital investi · Marge déployée · Rendement sur marge.
- **Sharpe et Sortino** sont calculés sur les rendements quotidiens par rapport à l'équité portée à chaque jour, annualisés, selon la même définition qu'Analytics, donc les deux écrans concordent.

## Analytics (lire le book)

Tout le book en huit onglets, sur les mêmes filtres que Breakdown :

- **Overview** : espérance en **R** et R total (sur les trades qui portent un stop planifié), risque par trade, Sharpe avec Sortino à côté, drawdown max avec les jours passés sous le sommet, jours de trading gagnés et perdus, jour moyen, série actuelle et meilleure, puis la **distribution des R** et le coût de vos erreurs étiquetées.
- **Distributions** : PnL net par durée de détention, heure d'entrée, jour de semaine d'entrée et taille de position, et le nombre de trades par tranche de profit. Quelle heure de votre journée paie vraiment.
- **Behavior** : ce que vous faites autour de l'avantage, et ce que cela coûte. Concentration du profit, taille après une série perdante, rythme après une perte, comment la journée se dégrade, et le trade après un gain contre le trade après une perte. Voir [Analyse du comportement](#behavior).
- **Market data** : les bougies derrière vos trades. MAE et MFE, efficacité de sortie, ce qui a été laissé sur la table, distance du stop en ATR, et résultats ventilés par régime de volatilité et par tendance à l'entrée. Voir [Enrichissement par données de marché](#market-data).
- **Open risk** : le seul onglet sur le présent. Ce qui est encore en jeu, où ce risque est concentré, et si cinq lignes ouvertes sont cinq paris ou un seul. Voir [Risque ouvert](#open-risk).
- **Scatter** : deux valeurs de trade quelconques tracées l'une contre l'autre (date, numéro de trade, net, PnL cumulé, rendement sur notionnel, R, durée de détention, taille...), colorées par résultat, sens, stratégie, ticker ou classe d'actifs, avec une droite de tendance et un zoom.
- **Breakdown** : performance groupée par stratégie, symbole, tag, classe d'actifs ou sens, avec trades, taux de réussite, net, espérance, R moyen et profit factor par ligne.
- **Compare** : ce jour, cette semaine, ce mois, ce trimestre, cette année ou une plage personnalisée face à la précédente, ligne par ligne (net, trades, taux de réussite, espérance, R moyen, profit factor, drawdown max, frais, jours de trading), au-dessus d'une bande des douze dernières périodes.

Les trades clôturés sans taux de change pour leur date sont comptés à voix haute plutôt qu'ignorés silencieusement.

## Analyse du comportement {#behavior}

Les statistiques de performance disent ce que le book a rapporté. **Behavior** dit comment vous y êtes arrivé, et laquelle de vos habitudes l'a payé. Mêmes trades clôturés que le reste d'Analytics, même barre de filtres, aucune configuration supplémentaire et aucune donnée de marché : il lit les trades que vous avez déjà consignés.

Cinq cartes, chacune répondant à une question.

### D'où vient le profit

La part du profit brut faite par vos cinq meilleurs trades, combien de gagnants il faut pour en faire la moitié, et à quoi ressemble le compte sans ces cinq. À côté, un indice de concentration : 0 signifie que chaque gagnant paie à peu près pareil, 1 signifie qu'un seul trade paie l'année. Gain moyen contre gain médian montre la même asymétrie sous un autre angle, et une courbe cumulée la dessine.

Le chiffre à regarder est le net sans les cinq meilleurs. S'il est négatif, l'avantage repose sur des cas extrêmes que vous ne pouvez pas planifier.

### Taille après une série perdante

Notionnel d'entrée médian groupé selon ce qui a précédé le trade : après un gain, après une perte, après deux, après trois ou plus. Chaque ligne porte son nombre de trades, son taux de réussite, son espérance, son R moyen et son net, donc l'escalade est chiffrée, pas seulement remarquée.

Augmenter la taille après deux pertes est l'habitude la plus coûteuse qu'un journal attrape. Une ligne plate ici est la discipline que la plupart des books perdent en premier.

### Rythme après une perte

L'écart médian entre une sortie et l'entrée suivante, comparé après un gain et après une perte. Un trade ouvert en bien moins que **votre propre** écart habituel juste après une perte est compté comme un revenge trade, puisqu'un scalpeur et un swing trader ne partagent pas la même horloge. Ces trades ont leur propre ligne : combien, ce qu'ils ont rapporté, et leur moyenne face au reste.

Les jours sont aussi comparés, un jour portant une perte contre un jour propre, sur le nombre de trades.

### Comment se passe la journée

Résultat moyen par rang du trade dans sa journée locale : premier, deuxième, troisième, quatrième et suivants, avec le R moyen par rang et ce que vaut chaque trade supplémentaire de la journée. Beaucoup de books gagnent leur argent avant le déjeuner et le rendent après. C'est ici que cela apparaît.

### Après un gain, après une perte

Le trade qui **suit** un résultat, jamais le résultat lui-même. Trades, taux de réussite, espérance, R moyen, taille médiane, risque moyen, détention médiane et écart médian, côte à côte, avec les trois écarts qui comptent (espérance, taille, détention) soulignés en dessous.

::: tip Les affirmations ont un plancher
La phrase en haut d'une carte n'est écrite qu'au-dessus d'un **plancher d'échantillon** (vingt trades clôturés dans la portée, huit de chaque côté d'une comparaison) **et** d'un seuil d'effet. En dessous de l'un ou l'autre, les cartes se dessinent quand même et sont étiquetées premier aperçu. Trois trades ne peuvent pas montrer une habitude.
:::

## Enrichissement par données de marché {#market-data}

Le journal de trades connaît votre entrée, votre sortie et votre stop. Il ne sait pas où est allé le prix pendant que vous étiez dedans, et c'est là que se trouvent la plupart des réponses utiles : vos stops sont-ils dans le bruit, quelle part de chaque mouvement vous avez réellement gardée, et dans quelles conditions de marché la stratégie fonctionne.

L'onglet **Market data** charge les bougies derrière vos propres trades et les mesure.

### À configurer une fois

Ouvrez **Sources** sur l'onglet :

- **Taille de bougie** : *Automatique* choisit l'unité de temps la plus grossière qui laisse encore environ vingt bougies dans une position typique, lue à partir de votre durée de détention médiane. Fixez-en une si vous préférez décider.
- **Récupération** : *Désactivée* ne mesure que ce qui est déjà stocké, *À la demande* télécharge quand vous cliquez, *Automatique* met en file la fenêtre manquante d'un nouveau trade de lui-même et vous notifie à l'arrivée.
- **Source par type d'actif** : actions, ETF, crypto, forex et futures choisissent chacun un [data connector](/fr/config/connectors) accordé au journal, ou *Automatique*, qui prend le premier connector accordé servant ce type.

Rien n'est téléchargé dans votre dos, et les téléchargements sont des tâches [Historical Data](/fr/modules/market-data#histdata) ordinaires : même file, même décompte de quota, même liste de tâches.

### Découvrir, télécharger, mesurer

Trois boutons, dans cet ordre.

- **Découvrir** lit ce dont les trades filtrés ont besoin face à ce que vous stockez déjà, et n'écrit rien. Par instrument vous obtenez les trades dans la portée, les barres en stock, les fenêtres manquantes et un statut : *prêt*, *partiel*, *manquant*, *pas de source*, *non pris en charge*, *contrat requis*.
- **Télécharger les manquants** met ces fenêtres en file et suit le lot. Seuls les trous sont demandés, et un trou est demandé aux barres plutôt que deviné à partir d'un écart : une série d'actions journalière manque chaque week-end, une série crypto 24h/24 jamais, donc aucune largeur d'écart ne fonctionne pour les deux.
- **Mesurer** parcourt chaque trade face à ses barres et stocke le résultat.

La mesure est **incrémentale**. Une mesure stockée est refaite quand le trade a été modifié, que de nouvelles bougies sont arrivées, ou que le grain a changé. *Tout re-mesurer* force toute la portée, pour quand vous changez la taille de bougie et voulez tous les trades sur le même pied.

### Nommer un contrat de futures

Une action porte le même nom partout. Un contrat de futures non, et votre ticker de journal nomme généralement la racine que vous tradez plutôt que le contrat que sert votre fournisseur de données.

Le journal demande donc une fois, au lieu de deviner. Un instrument qui en a besoin affiche *contrat requis*, et **Nommer le contrat** prend le symbole tel que votre source l'écrit : `MNQU6` (ce que montre TWS et que vous copiez), ou la racine avec son mois de contrat, `MNQ.202609`, ou `MNQ.202609@CME` quand la racine est cotée sur plusieurs places. Tout ce qui suit utilise ce symbole.

Les options sont signalées **non prises en charge** plutôt que rapprochées de leur sous-jacent. Mesurer un trade d'option contre les bougies de l'action produirait des chiffres qui ont l'air justes et ne veulent rien dire.

### Ce que vous obtenez

- **Excursions** : MAE et MFE moyens, en argent et en unités du risque planifié, avec le temps médian de l'entrée à chacun.
- **Efficacité de sortie** : la part du meilleur mouvement que vous avez réellement gardée, et ce qui a été laissé sur la table sur tous les trades mesurés.
- **Les stops sont-ils trop serrés** : distance médiane du stop en ATR à l'entrée, combien de stops sont sous un ATR, et combien de *gagnants* ont d'abord dépassé 80 % de leur risque. Un stop dans le bruit est un stop que le marché prend sur son chemin vers votre objectif.
- **Les objectifs sont-ils trop proches** : gagnants qui ont gardé moins de la moitié du mouvement offert, et ce qui s'affichait au meilleur point face à ce qui est rentré.
- **Quelle profondeur avant que ça marche** : le pire point de chaque trade réparti en R, de 0 à 0,25R jusqu'à plus de 1,5R. C'est ce qui vous dit où un stop doit se placer.
- **Par régime de volatilité** et **par tendance à l'entrée** : les mêmes statistiques ventilées calme / normal / volatil, et haussier / plat / baissier.

Les régimes sont des **terciles de votre propre book**, pas des seuils absolus. Un seuil absolu qualifierait chaque trade crypto de volatil et ne vous dirait rien sur le moment où votre stratégie fonctionne. En dessous de douze trades mesurés, aucun régime n'est étiqueté.

L'onglet Scatter les lit aussi : MAE contre R, efficacité contre durée de détention, le nuage coloré par régime.

## Risque ouvert {#open-risk}

Tous les autres onglets mesurent le passé. **Open risk** mesure ce qui est encore en jeu maintenant.

Une position est ouverte quand il reste de la quantité, et elle est comptée à ce **reste** : un trade allégé aux trois quarts porte un quart du risque, pas le risque avec lequel il s'est ouvert.

### Le titre

- **Exposition brute et nette**, en argent et en part du compte, longs et shorts additionnés puis compensés.
- **En jeu** : ce que vous perdez si chaque stop planifié est touché. Les positions sans stop consigné sont comptées séparément et nommées, puisque ce qu'elles risquent est inconnu, pas nul.
- **Résultat ouvert**, valorisé au dernier cours de clôture stocké, en disant combien de positions ont pu être valorisées.
- **Paris effectifs** : combien de positions indépendantes vos lignes représentent, par taille, et de nouveau à leurs corrélations mesurées.

Le tableau des positions les liste de la plus grosse à la plus petite, avec sens, taille ouverte, entrée, stop, dernier prix, valeur, ce qui est en jeu, sa part du total, résultat ouvert et jours de détention.

### Où se trouve le risque

Concentration par instrument, classe d'actifs, sens ou stratégie, calculée sur le **risque** quand des stops sont consignés et retombant sur la taille quand ils ne le sont pas (le panneau dit lequel). Un instrument qui porte la moitié de ce qui est en jeu est un fait sur votre book qu'aucune courbe d'équité ne montre.

### Sont-ce des paris séparés

Cinq lignes qui bougent ensemble sont une seule position à cinq fois la taille. Pour y répondre, l'onglet mesure la corrélation sur les **bougies journalières déjà en stock**, quel que soit le grain utilisé par l'enrichissement, et ne récupère jamais rien.

Trois nombres, et seul le troisième décrit votre book :

- **Additionné** : chaque stop touché à la fois, sommé.
- **Si indépendants** : ce que serait le risque si rien ne bougeait ensemble.
- **À ces corrélations** : ce que le book risque réellement.

Leur ratio est le facteur d'**empilement** : 1,0 signifie des paris véritablement séparés, plus haut signifie le même pari plusieurs fois. Les instruments sans bougies stockées sont nommés et exclus de la matrice plutôt que supposés.

Les avertissements se lisent comme des phrases : une paire qui bouge à 0,9, un instrument unique qui en porte trop, des positions sans stop, un book plus mince qu'il n'y paraît. Quand rien ne va mal, c'est dit aussi.

## Tags de discipline

Un **tag** est une règle que vous avez enfreinte ou respectée : *déplacé mon stop*, *pas de setup*, *augmenté la taille après une perte*. Cochez-les sur les trades concernés et Analytics les chiffre : combien de trades clôturés ont enfreint une règle, ce que ces trades rapportent en moyenne face aux trades propres, et l'écart entre les deux. C'est le **coût des erreurs**, en argent.

## PnL Calendar

Une grille mensuelle du PnL réalisé quotidien, vert pour les jours en hausse et rouge pour les jours en baisse, à l'échelle du plus gros jour du mois, avec les totaux hebdomadaires sur le côté. Cliquez sur un jour pour aller à ses trades.

Il lit aussi vos **routines de trading**. Rattachez les routines que suit une catégorie, sur une période, et chaque jour tradé porte un point : vert quand chaque routine due ce jour-là a été cochée, rouge quand aucune ne l'a été, ambre entre les deux. Un jour sans trade reste gris quoi que disent les routines, et un jour sans routine due n'a aucun point. Le survol nomme chaque routine avec sa propre marque.

Les routines elles-mêmes vivent dans [Trading Routines](/fr/modules/productivity#routines) et y sont cochées : le journal enregistre seulement lesquelles un book suit, donc la même habitude n'est jamais écrite ni cochée deux fois.

## Importer un livre de trades {#import-a-trade-book}

La vue **Import** prend un livre de trades que vous tenez ailleurs (un export de broker, un autre journal, un tableur) et le transforme en trades du journal. Pas de parseur par broker : CSV, TSV, JSON et Parquet passent tous par la même détection.

**Comment ça marche.** Déposez le fichier, le serveur propose un mappage, vous le vérifiez face à un aperçu de vrais trades, puis vous importez.

- La **détection** lit les en-têtes (en, fr, es, de, it, pt, plus le vocabulaire courant des brokers) *et* les valeurs elles-mêmes. Le délimiteur, le séparateur décimal et le jour-d'abord vs mois-d'abord des dates sont décidés par colonne. Une colonne qu'elle ne peut pas identifier avec confiance reste **non mappée** plutôt que devinée, et vous l'assignez vous-même.
- **Aperçu avant écriture.** L'étape d'analyse n'écrit rien : elle renvoie les trades construits, les totaux et les erreurs par ligne, et elle se relance à chaque modification du mappage, donc ce que vous voyez est exactement ce qui sera enregistré. Le P&L propre du fichier est recoupé avec celui calculé.
- **Forme des lignes.** Une ligne est soit un **aller-retour** (une ligne = un trade), soit une **exécution** (une ligne = une exécution). Les exécutions sont groupées par instrument en positions avec jambes d'entrée et de sortie ; ce qui est encore ouvert à la fin est importé comme trade ouvert.
- **Relevés empilés.** Un export qui range plusieurs tables dans un fichier (le style IBKR) est lu section par section, avec un sélecteur pour changer de table ou lire le fichier à plat.
- **Valeur du point.** Pour chaque ticker trouvé dans le fichier, l'import demande la valeur du point du contrat, puisqu'aucun export ne la porte. Elle est enregistrée avec le mappage.
- **Mappages.** Enregistrez un mappage et le fichier suivant de la même source est reconnu par l'empreinte de ses en-têtes et se mappe tout seul. Les colonnes que vous corrigez à la main sont aussi mémorisées.

::: tip Un mappage n'est pas un modèle
Un **modèle** de journal est le formulaire avec lequel vous consignez un trade à la main. Un **mappage** d'import dit quelle colonne d'un fichier étranger correspond à quel champ de trade. Ils sont listés séparément et jamais mélangés.
:::

### Récupérer depuis un broker

Avec un [compte broker](/fr/config/brokers) accordé au journal, **Récupérer depuis un broker** fait le même import sans fichier : choisissez le compte et une période, et les exécutions reviennent intégrées en positions, prévisualisées avant toute écriture. Il n'y a pas de mappage à vérifier, une API répond avec des champs typés. Relancer une période plus large actualise les positions déjà importées au lieu de les doubler.

**Annuler un import.** Chaque import est un **lot**, listé avec sa date, son fichier et son nombre de trades. *Annuler* supprime exactement les trades qu'il a créés. Les imports sont aussi dédoublonnés **par catégorie**, donc réimporter le même fichier ne change rien (le même relevé peut quand même alimenter deux catégories, puisqu'une catégorie est un book). *Oublier* un lot supprime cette protection et rend ses trades ordinaires de nouveau.

## Export et rapports

Depuis l'onglet Trades vous pouvez exporter vos données et générer un rapport de performance :

- **Export CSV** : les trades bruts, pour les tableurs ou les logiciels fiscaux.
- **Rapport périodique** : un résumé de performance hebdomadaire ou mensuel (taux de réussite, espérance, frais, répartition par stratégie et catégorie, courbe d'équité), rendu en **Markdown ou PDF**.

## Fonctionne avec

- **Tax Calculator** : charge votre PnL de journal réalisé pour une année fiscale, réparti en gains en capital / dérivés / crypto.
- **Historical Data** : les onglets Market data et Open risk lisent les bougies via les [data connectors](/fr/config/connectors) accordés au journal, et téléchargent ce qui leur manque comme des tâches ordinaires.
- **Trading Routines** : le PnL calendar montre, par jour tradé, si les routines suivies par la catégorie ont été cochées.
- **Dashboard** : un widget de trade rapide consigne un trade depuis la page d'accueil.
- **RemindMe** : ajoutez des rappels liés au journal (p. ex. revue hebdomadaire).
