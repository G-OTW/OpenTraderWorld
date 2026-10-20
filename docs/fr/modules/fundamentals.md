# Fundamentals

Un seul endroit pour lire l'économie et une société à partir des sources qui publient les chiffres : séries macro avec graphiques, états financiers des sociétés, dépôts SEC, transcriptions d'appels de résultats, positions d'ETF, calendrier de marché et données alternatives. Tout est stocké dans votre propre base de données, donc les graphiques, l'[Agent](/fr/modules/agent) et les autres modules le lisent sans redemander au fournisseur.

Fundamentals n'a pas de réglages de fournisseur propres. Chaque source est un **[data connector](/fr/config/connectors)** accordé au module : l'icône de prise dans l'en-tête de la page ouvre l'écran partagé des connectors. De nombreuses sources sont des organismes publics **sans clé** (SEC EDGAR, le Trésor américain, la BCE, Eurostat, la BRI, l'OCDE, le FMI, la Banque mondiale, la CFTC, FINRA, USAspending) ; les autres prennent une clé gratuite ou payante que vous apportez. Rien n'est récupéré tant que vous n'avez pas ajouté un connector et ne l'avez pas accordé à Fundamentals.

## Pages

| Page | Ce qu'elle montre |
|---|---|
| **Macro** | Vos séries par catégorie (croissance, inflation, emploi, taux, monnaie, enquêtes, immobilier, énergie, budget, positionnement), jusqu'à quatre sur un graphique, la courbe des taux du Trésor et les taux directeurs des banques centrales. |
| **Company** | Profil et métriques clés, états financiers, estimations, résultats, segments, dividendes et rachats, actionnariat, pairs, ESG et rémunération, dépôts et transcriptions. |
| **ETF** | Profil, principales positions, exposition par secteur et par pays. |
| **Events** | Résultats à venir, introductions en bourse, opérations sur titres et décisions de banques centrales. |
| **Documents** | Chaque dépôt et transcription stocké, avec recherche plein texte et lecteur de transcriptions. |
| **Alternative data** | Trades du Congrès, dépenses de lobbying, contrats fédéraux et brevets accordés. |
| **Library** | Onglets pour les séries et sociétés que vous conservez (filtrables), la priorité des sources, et quel fournisseur sert quelle famille de données. |

**Personnaliser** (en haut à droite) règle la densité, les sections affichées et leur ordre, page par page.

## Séries macro {#macro}

**Ajouter des séries** cherche dans le catalogue d'un fournisseur ou prend le code propre au fournisseur (`CPIAUCSL` sur FRED, `HICP/M.U2.N.000000.4D0.ANR` sur la BCE). Un code est vérifié auprès du fournisseur avant que rien ne soit stocké : un code inconnu est une erreur qui le nomme, jamais une série vide. **Ajouter un jeu de départ** ajoute en un clic une première sélection de séries américaines et de la zone euro.

| Fournisseur | Clé | Ce qu'il couvre |
|---|---|---|
| FRED | gratuite | La plupart des séries américaines (reflète aussi BLS, BEA et Census) |
| US Treasury | aucune | Courbe quotidienne des taux au pair, dette publique totale |
| ECB, Eurostat | aucune | Inflation, taux, monnaie, PIB, chômage de la zone euro |
| BIS | aucune | Taux directeurs des banques centrales, taux de change effectifs |
| OECD, IMF, World Bank | aucune | Indicateurs avancés, World Economic Outlook, données annuelles par pays |
| BLS, BEA, EIA, US Census | gratuite | Détail américain quand FRED est en retard ou n'a pas une série |
| CFTC | aucune | Commitments of Traders, positionnement net non commercial |

Une ligne est une **période** : une observation est stockée à la date de début de la période qu'elle couvre. Le graphique calcule les transformations à la lecture (niveau, glissement annuel, variation de période, différence, indice 100), donc rien de dérivé n'est jamais stocké. Le glissement annuel compare chaque valeur à celle datée d'un an plus tôt ; une période manquante affiche un trou plutôt qu'une comparaison avec le mauvais mois. L'ombrage des récessions suit les dates du NBER.

## Companies {#company}

Company, ETF et Alternative data partagent un **sélecteur de symboles** : vos favoris d'abord, puis les 15 ouverts le plus récemment. Sa recherche couvre chaque symbole stocké, plus les correspondances EDGAR pour les sociétés.

Tapez un ticker. Il est résolu dans la liste de tickers de SEC EDGAR ; un ticker qu'EDGAR ne connaît pas est une erreur, jamais une meilleure supposition. Ouvrir une société la stocke et récupère, en arrière-plan :

- les **États financiers** à partir des faits XBRL de la société, annuels et trimestriels. Les quatrièmes trimestres et les lignes de flux de trésorerie cumulées depuis le début de l'année sont dérivés par différence ; chaque ligne garde la balise sous laquelle elle a été déclarée.
- les **Dépôts** (10-K, 10-Q, 8-K, procurations...) avec un lien vers la source.
- les **Transactions d'initiés** analysées depuis le Form 4.

Les autres onglets lisent les agrégateurs que vous connectez, meilleure source d'abord. Un fournisseur dont l'offre exclut un jeu de données (une clé FMP gratuite et l'historique des résultats, par exemple) passe la main au suivant, et un connector accordé au module sans sa clé est ignoré. Pour le prix, l'historique le plus long l'emporte (les offres gratuites s'arrêtent souvent à un ou deux ans) :

| Données | Fournisseurs |
|---|---|
| Estimations, objectifs de cours, actions de notation | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Résultats (BPA estimé et réel, prochaine date) | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Segments | Financial Modeling Prep |
| Dividendes et fractionnements | EODHD, Massive, Financial Modeling Prep, Alpha Vantage |
| Détenteurs 13F | Financial Modeling Prep |
| Intérêt à découvert | FINRA, Massive |
| Pairs | Financial Modeling Prep, Finnhub |
| ESG et rémunération des dirigeants | Financial Modeling Prep, Finnhub |
| Transcriptions | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Prix et ratios de marché | tout connector de données de marché avec barres journalières d'actions |

**Library → Priorité des sources** liste chaque jeu de données ayant plus d'une source (transcriptions comprises) dans l'ordre où ses fournisseurs sont essayés. Choisissez un rang à côté d'un fournisseur pour l'y déplacer ; **Ordre par défaut** remet l'ordre de l'application. Le prix n'a pas d'ordre : l'historique le plus long l'emporte.

Chaque onglet indique quel fournisseur a répondu et quand, ou l'erreur qui nomme la solution (généralement un connector à ajouter ou une offre qui n'inclut pas le jeu de données). Une réponse est conservée et réutilisée jusqu'à ce qu'elle devienne périmée (quelques heures pour le calendrier, un jour pour les estimations, une semaine pour les détenteurs) ; **Actualiser** redemande maintenant.

Ouvrir une page ne dépense pas votre quota auprès d'un fournisseur qui vient de refuser : un qui a refusé le jeu de données (offre, symbole, limite de débit) est laissé tranquille un moment, de quelques minutes après une erreur réseau à une semaine après un refus d'offre, de même qu'un dont le quota, déclaré sur son connector, est épuisé. L'onglet le dit et indique le prochain essai automatique ; **Actualiser** interroge tous les fournisseurs à la fois.

**Suivre** une société pour qu'elle soit actualisée chaque jour et pour être prévenu de ses nouveaux dépôts.

### Transcripts {#transcripts}

L'onglet Transcripts liste les appels qu'un fournisseur détient pour la société ; le texte d'une transcription est récupéré à la première ouverture, découpé en tours de parole, remarques préparées séparées des questions-réponses, et consultable avec les autres documents.

## Alternative data {#alt}

La société est celle ouverte dans Company ; un ticker saisi ici est ouvert d'abord.

| Onglet | Fournisseurs |
|---|---|
| Trades du Congrès | Quiver Quant (les plus récents tous membres confondus, ou ceux d'une société), Finnhub premium (par société) |
| Lobbying | LDA.gov, Quiver Quant |
| Contrats publics | USAspending, Quiver Quant |
| Brevets | USPTO Open Data Portal (clé gratuite), Quiver Quant |

Les sources publiques connaissent une société par son **nom enregistré**, pas par son ticker. LDA.gov, USAspending et l'USPTO sont interrogés avec le nom qu'EDGAR stocke, et un enregistrement ne compte que si son nom est identique une fois la ponctuation et le suffixe juridique retirés (`Lockheed Martin Corp` correspond à `LOCKHEED MARTIN CORPORATION`, jamais à `Lockheed Martin Aculight`). Chaque onglet montre le nom auquel il a fait correspondre. Pour les contrats, le destinataire parent est utilisé, donc les filiales déposées sous lui comptent et une filiale enregistrée séparément (Amazon Web Services sous Amazon) non.

LDA.gov prend une clé gratuite (inscription sur lda.gov). Son pare-feu repousse les réseaux hors des États-Unis (un HTTP 403 qui le nomme) : joignez-le depuis une connexion américaine, ou utilisez Quiver Quant. Un rapport ne compte qu'une fois : un amendement remplace l'original, et les enregistrements de lobbyistes, qui ne portent aucune dépense, sont exclus. Une société qui fait du lobbying via une filiale sous un autre nom (JPMorgan Chase Holdings pour JPMorgan Chase) ne montre que les rapports déposés sous son propre nom.

## Couverture des fournisseurs {#coverage}

Quel fournisseur peut servir quelles données, comme dans **Library → Couverture des fournisseurs**. Une famille servie par plusieurs fournisseurs est essayée dans l'ordre de **Library → Priorité des sources**. Le prix et les ratios de marché viennent de tout connector de données de marché avec barres journalières d'actions (Alpha Vantage, EODHD, Massive, Yahoo, IBKR...), non listés ici.

| Fournisseur | Clé | Macro | COT | États financiers | Dépôts | Initiés | Estimations | Résultats | Segments | Dividendes | Détenteurs | Intérêt à découvert | Pairs | ESG | ETF | Calendrier | Transcriptions | Alt data |
|---|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| SEC EDGAR | aucune |  |  | ✓ | ✓ | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |
| FRED (St. Louis Fed) | gratuite | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Treasury | aucune | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| ECB Data Portal | aucune | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| Eurostat | aucune | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BIS | aucune | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| OECD | aucune | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| IMF | aucune | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| World Bank | aucune | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BLS | gratuite | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BEA | gratuite | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| EIA | gratuite | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Census | gratuite | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| CFTC | aucune |  | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| FINRA | aucune |  |  |  |  |  |  |  |  |  |  | ✓ |  |  |  |  |  |  |
| Financial Modeling Prep | offre gratuite |  |  |  |  |  | ✓ | ✓ | ✓ | ✓ | ✓ |  | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| Finnhub | offre gratuite |  |  |  |  |  | ✓ | ✓ |  |  |  |  | ✓ | ✓ |  | ✓ | ✓ | ✓¹ |
| Alpha Vantage | offre gratuite |  |  |  |  |  | ✓ | ✓ |  | ✓ |  |  |  |  | ✓ | ✓ | ✓ |  |
| EODHD | offre gratuite |  |  |  |  |  |  |  |  | ✓ |  |  |  |  | ✓ | ✓ |  |  |
| Massive (Polygon.io) | offre gratuite |  |  |  |  |  |  |  |  | ✓ |  | ✓ |  |  |  |  |  |  |
| USAspending | aucune |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| LDA.gov (lobbying) | gratuite |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| USPTO Open Data Portal | gratuite |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| Quiver Quant | payante |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |

¹ Finnhub ne sert les trades du Congrès que sur une offre premium.

Indicatif : les offres changent, et une offre gratuite peut exclure une famille (Financial Modeling Prep gratuit : pas de détenteurs 13F, de transcriptions ni de positions d'ETF ; Finnhub gratuit : pas d'ESG ni de trades du Congrès ; Alpha Vantage gratuit : 25 requêtes par jour). Consultez la page du fournisseur avant de payer.

## Actualisation automatique et notifications {#refresh}

Les séries stockées sont actualisées dès que leur fréquence indique qu'une nouvelle valeur peut être sortie : une série quotidienne deux fois par jour, une hebdomadaire ou mensuelle chaque jour, une trimestrielle tous les trois jours, une annuelle chaque semaine. Les sociétés suivies sont actualisées depuis EDGAR une fois par jour. Une source sans connector accordé est ignorée.

Une série avec une nouvelle période et une société suivie avec un nouveau dépôt (hors formulaires d'initiés) déclenchent une notification, poussée vers les [canaux](/fr/config/settings#notifications) accordés à Fundamentals (la cloche dans l'en-tête de la page).

## Widgets du dashboard {#dashboard}

Le [Dashboard](/fr/modules/dashboard) propose des séries et tableaux macro, des aperçus de sociétés, l'historique d'états financiers, des sociétés, des dépôts, les résultats à venir et des comparaisons de valorisation. Les cartes montrent la devise, la base de reporting et les dates ; leur actualisation ne lit que des données stockées. Les résultats à venir utilisent l'instantané de résultats stocké d'une société ou, quand il n'a pas de date future, le calendrier de marché stocké. La valorisation utilise des sociétés stockées sélectionnées manuellement ou les pairs stockés de son onglet Pairs. Les estimations et ratios manquants restent indisponibles plutôt que déduits.

## Recherche et agents {#search}

La recherche de la barre du haut, avec les titres de contenu activés, trouve les sociétés, séries et titres de documents stockés.

Les agents atteignent ce qui est stocké, en lecture seule, via la [passerelle MCP](/fr/config/ai-agents) une fois qu'un token reçoit **Fundamentals** : séries et observations, sociétés, états financiers, dépôts, transcriptions, transactions d'initiés et tout jeu de données stocké. Chercher chez un fournisseur, actualiser et ajouter des séries restent exclus : cela dépense votre quota de fournisseur.
