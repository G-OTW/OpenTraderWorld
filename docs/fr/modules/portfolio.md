# Portefeuilles et patrimoine

Des modules indépendants pour suivre ce que vous surveillez, ce que vous possédez, ce que cela vous coûte, et ce que pourrait être la facture fiscale.

## Watchlists {#watchlists}

Des listes nommées de symboles que vous voulez garder à l'œil : pas de positions, pas de registre, juste des cotations.

- **Ajoutez des symboles** par recherche (crypto via CoinGecko, actions/ETF via Yahoo), partez d'un **modèle sélectionné** (Crypto Top 10, Magnificent 7, ETF d'indices US, Semi-conducteurs), ou **importez un portefeuille de Portfolio Tracker**, où réimporter réconcilie au lieu de dupliquer.
- Chaque ligne montre le prix USD en direct, les **variations 24h / 3j / 7j / 30j**, une **sparkline 30 jours**, la place de cotation, et une **note** libre par symbole. Triez par n'importe quelle colonne, filtrez par nom.
- **Actualisation automatique** par liste, de chaque minute à quotidienne (15 min par défaut). La page estime le débit de requêtes et **avertit avant qu'un intervalle ne risque de déclencher la limitation des API gratuites**. Les cotations sont mises en cache côté serveur, donc rouvrir la page est instantané et ne sollicite jamais les fournisseurs.

### Sources de cotations personnalisées

Les sources publiques (CoinGecko / Yahoo) fonctionnent immédiatement sans configuration. Si vous avez votre propre compte de données de marché, branchez un **[data connector](/fr/config/connectors)**, le même compte de fournisseur partagé que [Historical Data](/fr/modules/market-data#histdata) et le graphique utilisent, créé une fois et accordé à Watchlists. Chaque connector porte ses propres identifiants (saisis, ou choisis dans le [Coffre](/fr/config/settings#vault)) et sa propre limite de requêtes.

- Une liste peut **épingler un connector comme source par défaut**, et chaque symbole peut la remplacer : *suivre la liste*, *auto*, ou un connector précis.
- Les **tickers fournisseur** par symbole (`BTCUSDT`, `AAPL.US`, …) sont dérivés automatiquement et restent modifiables quand un fournisseur nomme un symbole différemment.
- Une cotation qui échoue apparaît **sur sa propre ligne**, donc un mauvais symbole ne masque pas le reste de la liste.

::: warning Connaissez les limites de votre offre
Une liste adossée à une source personnalisée débloque des **intervalles d'actualisation de 5 s à 30 s**. C'est assez rapide pour épuiser rapidement une offre d'API : les appels en excès échouent et peuvent faire bloquer votre clé. Surveillez les compteurs dans **Paramètres → Débit API**.
:::

### Alertes de prix

Tout symbole peut porter des alertes, définies depuis la cloche de sa ligne. Chacune se lit comme une phrase que vous assemblez de gauche à droite, *me notifier quand BTC bouge de ±5 % à partir de maintenant* :

- **Un niveau** (prix au-dessus ou en dessous d'une valeur), ou **un mouvement** mesuré en **%** ou en **$**, à la hausse, à la baisse ou dans les deux sens.
- Un mouvement est mesuré **à partir de maintenant**, ou sur une **fenêtre glissante** (1h, 4h, 12h, 1j, 3j, 7j, 30j).
- **Une seule fois ou répétée**, avec un délai de réarmement (5 min à 1 j) pour qu'une seule oscillation ne se déclenche pas à chaque actualisation.
- **Destinations** : la boîte de réception de l'application plus tout [canal de notification](/fr/config/settings#notifications) que vous choisissez par alerte. Watchlists ne peut cibler que les canaux qui lui ont été accordés.

Les alertes sont évaluées **côté serveur dans la boucle d'actualisation**, donc elles se déclenchent page fermée et navigateur éteint.

### Description de liste

Une watchlist porte une description modifiable sous son nom, pour dire à quoi sert réellement la liste.

### Colonnes et vues

- **Choix des colonnes** : l'engrenage au bout de l'en-tête du tableau choisit les colonnes de chaque liste : capitalisation, volume, PER, rendement du dividende, RSI(14), plage 52 semaines, écart au plus haut 52 semaines, objectifs. Un champ que la source ne fournit pas affiche **n/d**, jamais 0 (Yahoo fournit la plage 52 semaines et le volume mais ni capitalisation, ni PER, ni rendement ; CoinGecko fournit capitalisation et volume).
- **Barre de position 52 semaines** : où se situe le prix dans sa plage annuelle, affichée par défaut. Les cryptos sur la source par défaut n'ont pas de plage 52 semaines ; une année de bougies journalières enregistrées la remplit si elle existe.
- **Carte thermique** : une tuile par symbole, dimensionnée par la capitalisation (taille égale si inconnue), colorée selon la variation 24 h, 7 j ou 30 j.
- **Comparer** : tous les symboles rebasés à 100 sur 1M, 3M ou 1A. 1M vient des cotations en cache ; 3M et 1A demandent des bougies journalières enregistrées dans [Historical Data](/fr/modules/market-data#histdata), et un symbole sans elles est listé comme non couvert avec la solution.
- **Corrélation** : la corrélation deux à deux des rendements journaliers sur la fenêtre, sur les jours où les deux symboles ont coté. Une paire avec moins de 10 jours communs affiche un tiret.

### Outils de ligne

- **Sections** : regroupez les lignes (cœur, candidats, sorties), repliez un groupe, glissez une ligne vers une autre section, renommez une section depuis son en-tête.
- **Drapeaux de couleur** : un point coloré avant le symbole, et un filtre par drapeau utilisé.
- **Objectifs** : un objectif d'achat et de vente par symbole avec l'écart à chacun ; la cloche à côté d'un objectif ouvre une alerte de niveau préremplie à ce prix.
- **Position virtuelle** : « si j'avais acheté N à P le D », avec le P/L et le % sous le prix. Rien n'est enregistré nulle part.
- **Événements** : prochaines dates de résultats, de détachement et de division par ligne et dans un bandeau À venir, lues dans ce que [Fundamentals](/fr/modules/fundamentals) a enregistré. Sans données enregistrées, le bandeau le dit.
- **Badge d'actualités** : combien d'éléments News & Research citent le symbole depuis le dernier « lu » de la liste ; un clic ouvre les actualités filtrées sur lui.
- **Ouvrir dans le graphique** : ouvre le symbole sur le graphique avec le ticker de son fournisseur. Une crypto sur la source CoinGecko par défaut demande d'abord une source de cotation fixée.
- **Ajouter au portefeuille** : ajoute le symbole à un portefeuille Portfolio Tracker choisi avec un premier achat prérempli au prix actuel.

### Coller des tickers et résumé quotidien

- **Coller** : la fenêtre Ajouter des symboles accepte une liste collée (virgules, espaces ou retours à la ligne). Chaque ticker n'est résolu que sur une correspondance exacte, rien n'est ajouté avant votre confirmation, un ticker ambigu liste ses correspondances à choisir et un inconnu dit comment le trouver.
- **Résumé quotidien des variations** : depuis les réglages de la liste (engrenage à côté du titre), une fois par jour à l'heure UTC choisie, les trois plus fortes hausses et baisses sur 24 h partent vers les canaux de notification choisis.

## Portfolio Tracker {#portfolios}

Valeur en direct de vos avoirs réels, un portefeuille par compte ou par thème.

- **Ajoutez des actifs** par recherche (cryptomonnaies ou actions/ETF) et enregistrez des **opérations d'achat/vente** (date, quantité, prix, frais, note). Les P/L réalisé et latent, le coût moyen et les pondérations sont calculés à partir du registre.
- **Devise de trade par actif** : chaque actif déclare la devise dans laquelle ses opérations sont saisies, donc une action achetée en EUR n'est pas consignée comme si elle était en USD. Les libellés du formulaire suivent cette devise. Les cotations spot restent en USD : le prix de revient et le P/L réalisé sont convertis à **la date propre de chaque opération** avec les taux de change du [Trading Journal](/fr/modules/journal), donc un achat d'il y a trois ans garde son taux historique. Une infobulle dans le formulaire explique d'où vient chaque prix.
- **Actualisation automatique** : une étape de réconciliation ponctuelle vérifie chaque avoir par rapport à sa source de prix ; corrigez ceux qui ressortent *non résolus* (ou marquez-les manuels) et activez la **mise à jour quotidienne automatique**, après quoi les prix s'actualisent en arrière-plan chaque jour.
- Par portefeuille : valeur, prix de revient, P/L latent/réalisé/total, meilleur et pire actif, **allocation** par actif ou classe, et un **graphique de la valeur dans le temps** (jour/semaine/mois/année) qui se remplit à mesure que les actualisations s'accumulent.
- La **description** reste modifiable après la création, à côté d'une note repliable de **thèse d'investissement** conservée avec le portefeuille, pour dire pourquoi vous détenez ce que vous détenez.

### Cash, revenus et coûts

Le registre n'est pas seulement des achats et des ventes. **Cash et revenus** dans l'onglet des opérations enregistre un
**dépôt**, un **retrait**, un **dividende**, des **intérêts**, un **coupon**, des **frais** ou une
**taxe**, chacun dans sa propre devise et avec des frais retenus optionnels. Un revenu peut nommer
l'avoir qui l'a versé, ou rien du tout quand il vient du compte lui-même.

À partir de ces lignes, le portefeuille obtient un solde de cash par devise, un revenu total, et une
vraie valeur nette (positions plus cash). Un cash négatif est affiché, jamais écrêté : il signifie une marge, ou un
registre auquel manquent ses dépôts, et les deux méritent d'être vus.

### Analysis

L'onglet **Analysis** répond sur la performance, le risque et l'exposition au même endroit. Sept vues
indépendantes, chacune ne demandant que les données dont elle a besoin, donc *Book* s'affiche instantanément sur une installation neuve
tandis que *Stress* paie pour les bougies.

Une vue qui ne peut pas répondre **dit pourquoi et quoi faire**. Elle n'affiche jamais un zéro qu'elle n'a
pas mesuré. Pas encore d'historique, un book trop court pour être annualisé, aucun benchmark choisi, pas de bougies
pour lui, aucune cible définie : chacun est une phrase et un bouton, pas un graphique vide.

| Vue | Nécessite | Répond à |
|---|---|---|
| **Book** | le registre, rien d'autre | valeur nette, investi face au cash, latent, réalisé, revenus, allocation par classe |
| **Performance** | historique quotidien | rendement, TRI, annualisé, apports nets, par fenêtre |
| **Risk** | historique quotidien | volatilité, drawdown, Sharpe, Sortino, Calmar, meilleures et pires périodes |
| **Benchmark** | historique quotidien et bougies du benchmark | ce que l'indice aurait rapporté à votre volatilité, alpha, bêta, capture |
| **Income & costs** | les lignes de cash du registre | revenus perçus, coûts payés, traînée annuelle, la courbe sans frais |
| **Allocation** | une allocation cible | actuel face à la cible, dérive, les trades qui la ferment |
| **Stress** | bougies quotidiennes par avoir | rejeu historique et chocs de facteurs, avec la part couverte |

Chaque vue utilise le même sélecteur de fenêtre : 1M, 3M, 6M, YTD, 1A, 3A, 5A, Tout, ou une plage
personnalisée. Une fenêtre plus longue que votre historique est signalée comme **non couverte**, avec les jours
qu'elle contient réellement, plutôt que présentée comme trois années complètes.

### Performance et risque

**Performance** rapporte un rendement pondéré par le temps à côté d'un TRI, et ils répondent à des questions
différentes. Le pondéré par le temps est ce que les investissements ont fait, corrigé des dépôts, car un apport
n'est pas une hausse. Le TRI est ce que **vous** avez obtenu, pondéré par l'argent, donc bien choisir le moment de vos achats n'apparaît
que là. Les apports nets figurent à côté, et un rendement sur moins d'un cinquième d'année
(environ 73 jours) n'est pas annualisé : multiplier six semaines par huit est une prévision, pas une mesure.

**Risk** lit la même courbe : volatilité, drawdown maximal, Sharpe, Sortino, Calmar, la part de
jours finis en hausse, meilleur et pire jour, mois, trimestre et année, et chaque drawdown plus profond
que 2 % avec **le temps qu'il a fallu pour le combler**. Un drawdown encore ouvert est marqué en cours, avec l'écart
au dernier plus haut et le nombre de jours écoulés.

Le facteur d'annualisation est **mesuré sur votre propre courbe**, pas supposé. Un book d'actions s'échange
environ 252 jours par an et un book crypto 365, et un book mixte n'est ni l'un ni l'autre. Sharpe et
Sortino utilisent le taux sans risque défini dans les réglages de mesure.

### Benchmark

Choisissez un instrument dont vous avez les bougies journalières (SPY, QQQ, BTCUSDT) et la page répond à la
seule question qui tranche un débat : **ce que cet indice aurait rapporté à votre
volatilité**, à côté de ce que vous avez réellement gagné. Battre l'indice en prenant trois fois son risque
n'est pas le battre.

En dessous : rendement total et annualisé pour les deux, volatilité, drawdown maximal et Sharpe côte à
côte, puis alpha, bêta, tracking error, ratio d'information et capture à la hausse et à la baisse.

Votre book est mesuré sur **les séances propres du benchmark**. Comparer un portefeuille 24h/24 7j/7 à un
indice jour par jour fait que chaque lundi de l'indice avale un week-end du vôtre, ce qui
sous-estime discrètement votre rendement.

### Revenus et coûts

Ce que vous avez perçu, ce que vous avez payé, et ce que le paiement vous a coûté. Dividendes, intérêts et
coupons d'un côté ; frais de trading et frais de compte de l'autre, avec la traînée annuelle exprimée comme part de
votre valeur nette moyenne.

La courbe est tracée deux fois : telle qu'elle s'est produite, et le même book sans les lignes de frais. Les frais
sont déjà dans votre prix de revient et votre cash, donc c'est une comparaison, pas une soustraction
que vous pourriez faire vous-même.

### Allocation cible

Dites quelle part de la valeur nette chaque panier doit détenir et de combien il peut dériver avant d'être considéré
hors bande, dans **Définir les cibles**. Une allocation doit totaliser 100 %, et un panier peut recevoir
le reste en un clic. Enregistrer une liste vide désactive la vue.

La vue montre alors l'actuel face à la cible par panier, l'écart, si chacun est dans
sa bande, et les **trades qui combleraient l'écart** : achetez tant de ceci, vendez tant de cela. Tout ce que vous détenez sans cible est listé plutôt qu'ignoré.

Affichage uniquement. Rien ici ne passe d'ordre, et rien ne se rééquilibre de lui-même.

### Stress testing

Deux moteurs, et tous deux vous disent quelle part de votre book le chiffre couvre.

Le **rejeu historique** applique le chemin quotidien réalisé de 2008, 2020, 2022, du T4 2018 ou du sommet
crypto de 2021 à ce que vous détenez aujourd'hui, avec les bougies propres des instruments sur ces dates. Pas
de modèle, pas de proxy. Un instrument qui n'existait pas alors n'a pas de chemin : il est **nommé et
exclu**, jamais remplacé par un indice.

Le **choc de facteur** déplace un vrai instrument (S&P 500, Nasdaq, taux, EUR/USD, pétrole, spreads
de crédit) et atteint chaque avoir via une sensibilité **mesurée**, ajustée sur ses propres
bougies. Un avoir sans bougies, avec un historique trop court ou un ajustement sans pouvoir explicatif
n'a **pas de bêta** : il atterrit dans *inexpliqué* avec son poids, et le titre se lit
"−11,8 % sur les 74 % du book qui ont pu être mesurés". Le cash a un bêta nul, ce qui est
généralement la seule diversification déjà en place.

Un choc de taux en points de base atteint une obligation via une duration écrite sur le scénario, donc le
chiffre se discute. Récession, flambée de l'inflation et élargissement du crédit sont livrés comme des
combinaisons modifiables de ces jambes.

Un panneau de disponibilité liste ce qui peut être stressé et ce qui ne le peut pas avant que vous lanciez quoi que ce soit, donc un
résultat mince est expliqué à l'avance plutôt qu'après.

### Historique quotidien

Chaque mesure ci-dessus sauf *Book* a besoin d'une courbe, et les instantanés ne commencent que le jour où vous activez
la tâche quotidienne. Un portefeuille que vous tenez depuis six ans serait sinon mesuré à partir de
mardi dernier. La courbe est donc **reconstruite à partir du registre et des bougies stockées**, jour par jour.

L'engrenage dans l'onglet Analysis ouvre la **Configuration de la mesure** :

1. **Nommez le ticker de bougies de chaque actif** et la devise dans laquelle ces bougies sont cotées. Un ticker
   de votre registre n'est pas toujours le symbole que sert votre fournisseur, et une action achetée en EUR
   valorisée avec des bougies USD est faussée par le taux de change.
2. **Télécharger les bougies manquantes**. Elles sont mises en file comme des tâches ordinaires [Historical Data](/fr/modules/market-data#histdata)
   via les connectors accordés aux portefeuilles. Si aucun n'en porte un de vos
   instruments, il est nommé, avec ce qu'il faut accorder.
3. **Reconstruire la courbe**. Elle rapporte les jours reconstruits, et les jours sautés parce qu'un avoir
   n'avait pas de bougie ce jour-là. Un jour qui ne peut pas être valorisé n'est pas stocké, plutôt que stocké faux.

Modifier une opération datée du passé marque la courbe **périmée à partir de cette date** et le dit.
Reconstruire reste votre décision. Actualiser un portefeuille télécharge aussi ce qui manque et prolonge
la courbe derrière, et vous prévient quand un broker ne peut pas servir l'un de vos instruments.

### Importer un registre d'opérations

**Importer** dans l'en-tête du portefeuille lit un export de broker, un tableur ou un autre tracker (CSV, TSV, JSON, Parquet) et transforme chaque ligne en une opération d'achat ou de vente. Même moteur de détection que l'[import du journal](/fr/modules/journal#import-a-trade-book) : en-têtes en six langues plus analyse des valeurs, conventions de délimiteur, de décimale et de date décidées par colonne, colonnes non identifiées laissées non mappées.

Trois choses qu'il refuse de deviner :

- **Ce qu'est un symbole.** Chaque symbole du fichier doit pointer vers un actif : un que vous détenez déjà dans ce portefeuille (associé automatiquement), un nouvel actif à créer, ou *ignorer*. Un symbole non résolu bloque l'import, et les actifs ne sont créés qu'une fois que vous confirmez.
- **Une ligne qui n'est ni un achat, ni une vente, ni un type qu'il reconnaît** est listée comme erreur de ligne au lieu d'être inventée en opération. Les dividendes, dépôts, retraits, frais et taxes **sont** reconnus, en six langues, et s'enregistrent tels quels. Si le fichier n'indique aucun type, définissez le défaut une fois pour tout l'import.
- **Un prix manquant** est dérivé de montant ÷ quantité et signalé, jamais rempli silencieusement.

Rien n'est écrit tant que vous ne validez pas l'aperçu. Chaque import est un **lot**, annulable en entier (les actifs créés restent), et dédoublonné **par portefeuille**, donc réimporter le même fichier ne change rien.

### Importer des avoirs depuis un broker

**Depuis un broker** dans l'en-tête du portefeuille lit le bilan d'un [compte broker](/fr/config/brokers) au lieu d'un fichier : l'écart avec votre registre est montré ligne par ligne, et chaque ligne que vous acceptez écrit l'unique opération qui met le portefeuille en accord. Le prix de revient est utilisé là où le broker en publie un (Interactive Brokers) et demandé là où il n'en publie pas (les exchanges crypto publient une quantité et rien d'autre).

### Opérations sur titres, lots et transferts

**Opérations sur titres.** Le groupe **Opération sur titres** du formulaire de trésorerie enregistre une division, un regroupement ou une scission sur une ligne. Une division multiplie les unités et divise le coût unitaire par le même ratio ; rien n'est réalisé. Une scission transfère la part de prix de revient que vous indiquez vers la nouvelle ligne, avec les dates d'achat de la société mère, qui garde ses unités.

**Lots fiscaux.** Cliquez sur un symbole pour ouvrir la ligne. **Lots** liste chaque achat ouvert avec son coût, sa valeur, ses jours de détention et sa durée courte ou longue. La méthode d'appariement, le seuil de détention et la fenêtre anti-lavage viennent de votre premier profil [Tax Calculator](#taxcalc) ; sans profil, FIFO s'applique sans durée ni règle anti-lavage, et l'écran le dit.

**Transfert entre portefeuilles.** **Transfert** dans la ligne déplace des unités vers un autre portefeuille avec leurs lots, leur prix de revient et leurs dates d'achat. Rien n'est réalisé d'un côté comme de l'autre ; l'actif de destination est créé s'il manque.

### Revenus, thèse et classement

**Calendrier des dividendes.** L'onglet **Revenus** prévoit le revenu annuel par ligne (unités × montant par versement × versements par an), affiche le rendement sur coût, les prochaines dates de détachement et de paiement, et les revenus enregistrés sur douze mois à côté de la prévision. Les sources de cotation ne fournissent pas les dividendes : vous saisissez montant, fréquence et dates dans la ligne, et une ligne sans données apparaît *sans estimation*. Le Dashboard propose une carte **Upcoming income**.

**Thèse et date de revue.** Chaque ligne porte sa propre thèse et une date de revue. Les thèses à revoir dans la semaine s'affichent en bandeau sur la page des portefeuilles.

**Secteur et pays.** Saisis dans la ligne, ou lus dans [Fundamentals](/fr/modules/fundamentals) pour une action connue. Un ETF reste non classé tant que vous ne le saisissez pas. Le donut d'allocation regroupe par secteur ou pays, et la part non classée forme sa propre tranche.

### Entre portefeuilles

**Tous les portefeuilles.** Dès deux portefeuilles, un bloc au-dessus de la liste additionne tout dans une devise : valeur, coût, P/L, liquidités, symboles détenus dans plusieurs portefeuilles avec leur poids cumulé, devises de cotation et plus grosses lignes.

**Comparer.** Analysis → **Comparer** trace ce portefeuille et un autre sur une même courbe, pondérés par le temps et ramenés à 100 le premier jour commun.

### Attribution et objectifs

**Contribution.** Sous le tableau de performance, des barres montrent ce que chaque ligne a apporté au rendement de la fenêtre, et au plus fort drawdown, à partir des clôtures quotidiennes. Les contributions totalisent le rendement du portefeuille reconstruit. Une ligne sans clôtures quotidiennes sur la fenêtre est nommée et les barres ne sont pas tracées.

**Moins-values.** Analysis → **Moins-values** liste les lots en perte, du pire au moindre, avec la fenêtre anti-lavage du régime et un signal sur tout lot acheté dedans. **Envoyer vers Tax Calculator** l'ouvre avec la perte préremplie.

**Objectifs.** Analysis → **Projection** prend une valeur cible, une date et un versement mensuel, et affiche la progression, le versement nécessaire au rendement mesuré du portefeuille et un cône de 1000 trajectoires tirées de son rendement et de sa volatilité mesurés. Une courbe de moins d'un cinquième d'année est refusée plutôt qu'extrapolée.

### Plans et notifications

**Ordres prévus.** **Plan** dans la ligne enregistre un achat ou une vente prévu à un prix. La cotation de l'actif est vérifiée chaque heure ; le plan est marqué atteint une fois et peut notifier. **L'enregistrer** ouvre le formulaire d'opération prérempli.

**Alertes et résumé.** **Alertes** dans l'en-tête du portefeuille règle la part maximale d'une ligne (une ligne au-dessus est signalée), une notification au dépassement, une notification quand une classe cible sort de sa bande (aussi dans la fenêtre des cibles), et un résumé quotidien ou hebdomadaire avec valeur, variation, trois plus fortes variations et revenus depuis le précédent. Chaque alerte part une fois par dépassement et se réarme au retour dans les limites. Les messages vont aux [canaux de notification](/fr/config/settings#notifications) autorisés pour Portfolio Tracker.

## MyWealth {#wealth}

Valeur nette de **tout** : comptes de courtage, immobilier, crypto, cash, objets de valeur. Là où Portfolio Tracker suit des avoirs cotés en direct, MyWealth suit tout actif que vous valorisez vous-même.

- Ajoutez des actifs avec un nom, un type, une devise et une catégorie, puis **enregistrez des mises à jour de valeur** au fil du temps (prix × quantité, ou une valeur directe, avec une note). L'historique est modifiable.
- **Graphique de valeur nette** par mois ou année, plus une ventilation par catégorie. Multi-devises avec la même gestion du change que le journal (les actifs sans taux sont exclus et signalés).
- **Modèles**, comme ceux du journal : des champs réservés prix/quantité alimentent la valeur, des champs personnalisés portent des notes par révision.
- **Possédé ou dû** : un actif peut être un **passif** (un crédit immobilier, un prêt), donc le titre est une vraie valeur nette. La page montre ce que vous possédez et ce que vous devez avant de les compenser.
- **Lier un portefeuille** au lieu de le copier : un portefeuille lié est lu en direct depuis le tracker à chaque fois, donc sa valeur dans votre valeur nette n'est jamais une copie périmée.
- **Âge de la valorisation** : dites à un actif à quelle fréquence il doit être revalorisé et la page nomme ceux qui ont dépassé ce délai, le plus ancien d'abord. Une maison valorisée il y a trois ans est fausse en silence, et c'est ce qui rompt le silence. Un portefeuille lié ne devient jamais périmé : il est lu, pas mémorisé.
- **Échéancier de prêt** : indiquez le montant, le taux annuel, la durée et la date de début (et la mensualité si celle du prêteur diffère). Le solde suit la formule d'amortissement, une valeur par échéance est écrite pour vous, et Mettre à jour liste les échéances restantes avec intérêts et capital.
- **Associer un actif à sa dette** : un actif détenu peut nommer le passif qui le grève (une maison et son prêt) ; son détail montre la valeur nette et le ratio prêt / valeur.
- **Dérive annuelle** : un taux supposé (+3 % pour l'immobilier, -15 % pour une voiture) estime la valeur après la dernière évaluation réelle. Le graphique la trace en pointillés, elle n'est jamais enregistrée comme évaluation, et l'en-tête indique la part estimée.
- **Compte de courtage et adresse de portefeuille** : l'onglet Compte de courtage du dialogue de liaison ajoute la valeur liquidative d'un compte comme actif, lue avec ses propres identifiants (le compte doit être autorisé pour MyWealth dans Paramètres, Courtiers). Un actif Adresse de portefeuille lit une adresse BTC ou ETH publique sur la chaîne, valorisée en USD. Les deux sont lus au plus une fois par heure et gardés comme valeur du jour ; une lecture en échec affiche son erreur sur la ligne, jamais un zéro.
- **Objectifs** : valeur nette et date visées, dépenses mensuelles. L'onglet montre la croissance mesurée sur votre courbe (12 mois d'historique requis), la date projetée, l'épargne sur 12 mois hors marchés et dérive, le taux d'épargne et le nombre FIRE (25 × dépenses annuelles).
- **Sous le graphique** : Variations répartit la variation d'une période par catégorie en réévaluation, ajouts et retraits ; Avant / maintenant met le tableau des catégories d'il y a 1 mois, 1 an ou 5 ans à côté d'aujourd'hui ; Liquidité divise les catégories marquées liquides par les dépenses mensuelles plus la prévision des Abonnements ; Cibles fixe une part et une marge par catégorie et montre l'écart, avec le calcul des cibles du Portfolio Tracker.
- **Valeur nette réelle** : le bouton Réel déflate la courbe par un indice des prix chargé dans Fundamentals (CPI américain par défaut, un autre dans Paramètres). Sans indice, le graphique le dit et propose de charger le CPI américain depuis FRED.
- **Propriétaire** : attribuez chaque actif à un membre du foyer, filtrez par propriétaire et voyez la répartition dans la vue Propriétaires du graphique.
- **Documents** : joignez un acte, un contrat ou un relevé (10 Mo chacun) à un actif ou à une de ses évaluations.
- **Avis d'évaluations périmées** : dans Paramètres, un avis hebdomadaire liste les actifs dont l'évaluation dépasse l'intervalle de révision et l'envoie par les canaux autorisés pour MyWealth.
- **Export et calculateur d'impôt** : le menu de l'en-tête exporte actifs et évaluations en CSV ; Calculateur d'impôt additionne les catégories choisies et ouvre le calculateur avec l'assiette de l'impôt sur la fortune remplie. La démo refuse les exports et les téléchargements de documents.

## Managers' Portfolios {#mportfolios}

Parcourez les **portefeuilles 13F des superinvestisseurs** : ce que détiennent les gérants de fonds célèbres, tailles de positions, activité récente, valeur déclarée vs actuelle, fourchettes sur 52 semaines. Filtrez par gérant ou par ticker (*qui détient AAPL ?*).

Comme les données 13F changent chaque trimestre, vous pouvez **enregistrer des instantanés** de n'importe quel portefeuille et comparer dans le temps.

## Tax Calculator {#taxcalc}

Estimation approximative des impôts de trading et d'investissement. **Pas un conseil fiscal.**

- Les **profils** partent d'un **régime fiscal** par pays et par année fiscale : France (flat tax, ou professionnel), Allemagne, Royaume-Uni, États-Unis, Australie, Suisse, Espagne et Italie. Un régime fixe le calendrier de l'année fiscale, la méthode de coût de revient (prix moyen pondéré, FIFO ou pool britannique, modifiable par profil), la règle anti-wash quand le pays en a une, les abattements et les taux, et affiche ses sources et la date de vérification de ses règles. Les **taux personnalisés** couvrent tout autre pays : taux forfaitaire ou marginal, prélèvements sociaux, abattements sur plus-values et dividendes, abattement optionnel selon la durée de détention. Tout profil peut ajouter des **barèmes d'impôt sur la fortune** (p. ex. CH, ES, NO).
- **Report des moins-values** : enregistrez le résultat net de chaque année fiscale par pool (valeurs mobilières, crypto...). Ce qui reste reportable sur l'année est calculé à partir de ces lignes, la perte la plus ancienne d'abord, et abandonné après la durée de report du régime.
- Saisissez les chiffres en mode **Résumé** (valeur de début/fin, apports, retraits, part réalisée) ou en mode **Détaillé** (plus-values en capital, plus-values sur dérivés, plus-values crypto, dividendes, intérêts, pertes antérieures reportées).
- **Charger le Trading Journal** : avec le journal installé, un clic charge le PnL réalisé d'une année fiscale, réparti en gains en capital / dérivés / crypto, converti au taux de change de fin d'année.
- **Depuis un broker** : avec un [compte broker](/fr/config/brokers) accordé au calculateur fiscal, une année fiscale est lue directement depuis le compte, intégrée en positions clôturées et totalisée par ligne de formulaire, chaque cession étant convertie au taux de sa propre date de sortie.
- Les résultats montrent l'impôt estimé avec une ventilation par élément (imposable, abattement, base, taux) et le taux effectif. Enregistrez des scénarios dans l'historique pour comparer.

## Subscriptions {#subscriptions}

Chaque coût récurrent dans une liste (outils de trading, flux de données, streaming) avec prix, devise, fréquence de facturation (hebdomadaire/mensuelle/trimestrielle/annuelle) et catégorie.

Vous obtenez des **graphiques de dépenses** mensuels/annuels (groupés ou par abonnement), l'**équivalent mensuel** de chaque abonnement, les prochaines dates de facturation, et les totaux du mois prochain. Mettez un abonnement en pause pour le garder listé sans le compter.

### Cycle de vie, rappels et budgets

- **Historique des prix** : changer un prix ou une devise l'enregistre. Les détails de la ligne montrent le prix en escalier, et le graphique mensuel trace une ligne pointillée « hausse des coûts » : ce que coûtait chaque mois tout ce qui tournait, au prix d'alors.
- **Rappels de renouvellement** : cochez « Me rappeler avant la facturation », réglez les jours et choisissez les canaux (aucun = tous les canaux autorisés pour Subscriptions). Chaque date de facturation est rappelée une fois. Un rappel demande une date de début ou une fin d'essai.
- **Essais gratuits** : cochez « Essai gratuit » et indiquez sa date de fin. La facturation commence ce jour-là, la ligne porte un badge, et les essais sur le point de basculer sont listés en premier.
- **Résilier** : le bouton stop fixe une date de fin. L'abonnement sort de tous les totaux à partir de ce jour, garde son historique, et le panneau compte ce qu'il a fait économiser depuis. Filtre en cours / résiliés / tous. La suppression reste pour les erreurs.
- **Passage à l'annuel** : pour un forfait facturé plus souvent, saisissez le prix annuel. La colonne « Gain en annuel » montre 12 mois au rythme actuel moins le prix annuel.
- **Note d'usage** : une bannière demande de noter chaque service en cours de 1 à 5 chaque trimestre. La vue « Ça vaut le coup ? » classe les services notés par coût mensuel par point, les plus chers pour leur usage en premier.
- **Budgets** : fixez un plafond mensuel par catégorie dans la devise d'affichage. Une catégorie au-dessus de son plafond apparaît dans le panneau et notifie une fois par mois.
- **Calendrier** : la vue Calendrier montre les dates et montants facturés du mois. « Exporter .ics » télécharge les 12 prochains mois en événements de calendrier (refusé dans la démo).
- **Moyen de paiement et expiration de carte** : indiquez la carte qui paie, filtrez dessus, et voyez un badge quand elle expire sous 60 jours ou a expiré.
- **Forfaits partagés** : indiquez votre part en % d'un forfait famille. Les prix, l'équivalent mensuel et tous les totaux utilisent votre part.
- **Prévision** : l'onglet Prévision montre les facturations des 12 prochains mois civils, mois par mois, avec la moyenne mensuelle. MyWealth lit les mêmes chiffres.
