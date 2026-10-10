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

Chaque vue utilise le même sélecteur de fenêtre : 1M, 3M, 6M, YTD, 1A, 3A, 5A, tout, ou une plage
personnalisée. Une fenêtre plus longue que votre historique est signalée comme **non couverte**, avec les jours
qu'elle contient réellement, plutôt que présentée comme trois années complètes.

### Performance et risque

**Performance** rapporte un rendement pondéré par le temps à côté d'un TRI, et ils répondent à des questions
différentes. Le pondéré par le temps est ce que les investissements ont fait, corrigé des dépôts, car un apport
n'est pas une hausse. Le TRI est ce que **vous** avez obtenu, pondéré par l'argent, donc bien choisir le moment de vos achats n'apparaît
que là. Les apports nets figurent à côté, et un rendement de moins de deux mois n'est pas
annualisé : multiplier six semaines par huit est une prévision, pas une mesure.

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

## MyWealth {#wealth}

Valeur nette de **tout** : comptes de courtage, immobilier, crypto, cash, objets de valeur. Là où Portfolio Tracker suit des avoirs cotés en direct, MyWealth suit tout actif que vous valorisez vous-même.

- Ajoutez des actifs avec un nom, un type, une devise et une catégorie, puis **enregistrez des mises à jour de valeur** au fil du temps (prix × quantité, ou une valeur directe, avec une note). L'historique est modifiable.
- **Graphique de valeur nette** par mois ou année, plus une ventilation par catégorie. Multi-devises avec la même gestion du change que le journal (les actifs sans taux sont exclus et signalés).
- **Modèles**, comme ceux du journal : des champs réservés prix/quantité alimentent la valeur, des champs personnalisés portent des notes par révision.
- **Possédé ou dû** : un actif peut être un **passif** (un crédit immobilier, un prêt), donc le titre est une vraie valeur nette. La page montre ce que vous possédez et ce que vous devez avant de les compenser.
- **Lier un portefeuille** au lieu de le copier : un portefeuille lié est lu en direct depuis le tracker à chaque fois, donc sa valeur dans votre valeur nette n'est jamais une copie périmée.
- **Âge de la valorisation** : dites à un actif à quelle fréquence il doit être revalorisé et la page nomme ceux qui ont dépassé ce délai, le plus ancien d'abord. Une maison valorisée il y a trois ans est fausse en silence, et c'est ce qui rompt le silence. Un portefeuille lié ne devient jamais périmé : il est lu, pas mémorisé.

## Managers' Portfolios {#mportfolios}

Parcourez les **portefeuilles 13F des superinvestisseurs** : ce que détiennent les gérants de fonds célèbres, tailles de positions, activité récente, valeur déclarée vs actuelle, fourchettes sur 52 semaines. Filtrez par gérant ou par ticker (*qui détient AAPL ?*).

Comme les données 13F changent chaque trimestre, vous pouvez **enregistrer des instantanés** de n'importe quel portefeuille et comparer dans le temps.

## Tax Calculator {#taxcalc}

Estimation approximative des impôts de trading et d'investissement. **Pas un conseil fiscal.**

- Les **profils** partent de **modèles par pays** (particulier ou professionnel) et restent entièrement modifiables : taux marginal d'imposition, prélèvements sociaux, abattements sur plus-values et dividendes, **barèmes d'impôt sur la fortune** optionnels (p. ex. CH, ES, NO), paliers d'abattement long terme.
- Saisissez les chiffres en mode **Résumé** (valeur de début/fin, apports, retraits, part réalisée) ou en mode **Détaillé** (plus-values en capital, plus-values sur dérivés, plus-values crypto, dividendes, intérêts, pertes antérieures reportées).
- **Charger le Trading Journal** : avec le journal installé, un clic charge le PnL réalisé d'une année fiscale, réparti en gains en capital / dérivés / crypto, converti au taux de change de fin d'année.
- **Depuis un broker** : avec un [compte broker](/fr/config/brokers) accordé au calculateur fiscal, une année fiscale est lue directement depuis le compte, intégrée en positions clôturées et totalisée par ligne de formulaire, chaque cession étant convertie au taux de sa propre date de sortie.
- Les résultats montrent l'impôt estimé avec une ventilation par élément (imposable, abattement, base, taux) et le taux effectif. Enregistrez des scénarios dans l'historique pour comparer.

## Subscriptions {#subscriptions}

Chaque coût récurrent dans une liste (outils de trading, flux de données, streaming) avec prix, devise, fréquence de facturation (hebdomadaire/mensuelle/trimestrielle/annuelle) et catégorie.

Vous obtenez des **graphiques de dépenses** mensuels/annuels (groupés ou par abonnement), l'**équivalent mensuel** de chaque abonnement, les prochaines dates de facturation, et les totaux du mois prochain. Mettez un abonnement en pause pour le garder listé sans le compter.
