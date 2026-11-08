# Paper trading

Un **compte de paper trading** est un broker géré par OpenTraderWorld lui-même. Les ordres qu'on lui envoie s'exécutent sur le prix en direct du graphique d'où ils partent, contre votre propre trésorerie simulée, avec les commissions, le slippage et le levier que vous fixez. Pas de clé, pas de place de marché, pas d'argent réel.

Il se trade depuis le graphique exactement comme un vrai broker : même ticket, même barre rapide, mêmes lignes d'ordres et de positions, mêmes onglets de panneau et même journal d'audit. Tout cela est décrit dans [Trader depuis le graphique](/fr/config/trading) ; cette page couvre ce qui est propre au paper.

## En créer un

**Paramètres → Brokers → Ajouter un compte**, broker **Paper trading**. Aucun identifiant. Les réglages (libellés affichés en anglais par l'application) :

| Réglage | Défaut | Signification |
|---|---|---|
| **Account currency** | `USD` | La devise de la trésorerie et des résultats, 3 à 5 lettres |
| **Starting balance** | `100000` | Inscrit comme premier dépôt |
| **Commission per order** | 0 | Un montant fixe par exécution |
| **Commission per unit** | 0 | Par action ou contrat |
| **Commission (% of value)** | 0 | `0.1` signifie 0,1 % de la valeur traitée |
| **Minimum commission** | 0 | Une exécution ne coûte jamais moins |
| **Slippage (basis points)** | 0 | `5` signifie 0,05 %, en votre défaveur |
| **Leverage** | 1 | 1 correspond à un compte au comptant |
| **Short selling** | yes | `no` refuse une vente sans rien à vendre |
| **Fill on** | `quote` | `quote` exécute au bid et à l'ask, `last` au dernier cours |

Les nombres doivent être positifs ou nuls, et le levier au moins égal à 1. Les réglages sont relus à chaque exécution : une modification s'applique dès la suivante.

Un nouveau compte paper a **Tous les modules** et **Trading** activés. Le trading sur un compte paper ne demande aucun mot de passe et n'est jamais derrière le [verrou de trading](/fr/config/trading#trading-lock).

**Tester la connexion** répond avec l'equity, la trésorerie, les positions ouvertes et les ordres actifs du compte.

## Trésorerie

Le panneau **Compte paper**, sous le compte dans Paramètres → Brokers, a un champ **Montant** et trois boutons :

- **Déposer** ajoute le montant à la trésorerie.
- **Retirer** le retire, dans la limite de la trésorerie détenue (pas de l'equity).
- **Réinitialiser le compte** supprime chaque ordre, exécution et mouvement de trésorerie, puis repart d'un seul dépôt : le montant saisi, ou le solde de départ si le champ est vide. Les réglages sont conservés, et les trades déjà importés dans le journal restent.

Le panneau montre aussi le total déposé, et les 50 derniers ordres qui ont cessé d'être actifs avec leur statut (exécuté, annulé, rejeté, expiré) et la raison. **C'est dans cette liste qu'un rejet est expliqué.**

## Comment les ordres s'exécutent

### Sur quel prix

Un ordre s'exécute sur le **flux en direct du data connector du graphique** : le connecteur d'où vient l'ordre, ou le connecteur du fournisseur autorisé pour les Graphiques. Le moteur tourne sur le serveur : **les ordres continuent de s'exécuter graphique fermé**, et s'arrêtent quand l'application est éteinte.

- Un instrument sans flux en direct est refusé au passage de l'ordre. Un indice aussi : il n'y a pas de marché à traiter.
- Le moteur suit les barres en direct les plus fines du fournisseur (1 minute si disponible) et ses cotations quand le flux en publie. Seuls les prix imprimés après l'activation de l'ordre comptent.
- Avec **Fill on: quote**, les achats s'exécutent à l'ask et les ventes au bid, tant que la cotation a moins de 15 secondes. Sinon, et avec **last**, les deux s'exécutent au dernier cours.
- Une barre de plus de 2 minutes n'est pas utilisée. Un ordre au marché envoyé marché fermé attend le premier prix frais au lieu d'être rejeté.
- Si le flux s'arrête, il est relancé toutes les 10 secondes et les ordres attendent. Si le data connector est supprimé, ses ordres ne peuvent plus s'exécuter.

### Par type d'ordre

| Type | S'exécute quand | À |
|---|---|---|
| **Market** | au prix suivant | ce prix, plus le slippage |
| **Limit** | le marché atteint la limite | la limite, ou mieux si le marché l'a déjà dépassée. Sans slippage |
| **Stop** | le marché atteint le stop | le stop s'il est touché, le marché en cas de gap, plus le slippage |
| **Stop limit** | le stop est atteint, puis la limite | comme un limit, éventuellement sur le même prix |
| **Trailing stop** | le marché revient de la distance de suivi depuis son meilleur prix depuis le début du stop (le plus haut derrière une vente, le plus bas derrière un achat) | comme un stop |
| **MIT** | le marché arrive au déclencheur par le côté du limit | le marché, plus le slippage |
| **LIT** | le déclencheur, puis la limite | comme un limit |

- **Pas d'exécution partielle** : un ordre s'exécute en entier, sans contrôle de volume ni de liquidité.
- Un **trailing stop** avec un prix de départ attend que le marché l'atteigne (y monter derrière une vente, y descendre derrière un achat), puis suit à partir de ce prix. Son meilleur prix est conservé après un redémarrage.
- **Reduce only** : placé seul, il est refusé sauf si le compte détient une position de l'autre côté. À son exécution, il est plafonné à ce qui est détenu alors, et annulé quand il ne reste rien à réduire.
- **Post only** : un limit refusé au placement quand il serait exécuté aussitôt contre le bid ou l'ask courant (le dernier cours sans cotation).
- Pas d'iceberg, de choix du déclencheur, de hors séance régulière, de close-position ni d'algo : un prix en direct sans profondeur, séance ni mark n'a rien sur quoi les jouer, le ticket ne les propose donc pas.
- Le prix d'exécution est arrondi au tick de l'instrument quand il est connu.
- **Commission** par exécution = par ordre + par unité × quantité + % × valeur traitée, relevée au minimum. Elle est prélevée dans la devise de l'instrument.

### Durée de validité

- **Day** (par défaut) expire au prochain **minuit UTC**, pas à la clôture de la bourse.
- **GTC** n'expire jamais. **GTD** expire à la date et l'heure données.
- **IOC** et **FOK** sont annulés si le premier prix qu'ils rencontrent ne les exécute pas. Marché fermé, ils attendent ce premier prix. Le paper exécute en entier, les deux finissent donc pareil.

L'expiration est vérifiée environ toutes les 10 secondes ; l'ordre apparaît alors comme expiré.

### Brackets et OCO

- Un **take profit** ou un **stop loss** attend l'exécution de son entrée, puis devient actif. Si l'entrée est annulée, rejetée ou expire, ses sorties sont annulées.
- Dans un **OCO**, une exécution annule l'autre ordre du groupe, et ses sorties.
- Les prix, la quantité et la durée de validité d'un ordre actif se modifient. **Clôturer la position** envoie un ordre au marché GTC, puis annule les sorties de la position.

## Fonds et levier

- Une position immobilise sa valeur ÷ levier. Les futures immobilisent leur marge par contrat à la place (voir plus bas), et ignorent le levier.
- Un ordre qui ouvre ou augmente une position est vérifié au passage puis au prix d'exécution, commission comprise. Il est refusé si le compte immobiliserait plus que son equity :

  > not enough funds: the positions would tie up X USD and the account's equity is Y USD

- Un ordre qui réduit une position est toujours accepté.
- Avec **Short selling** à `no`, une vente qui passerait à découvert est refusée.
- Il n'y a **ni appel de marge ni liquidation forcée** : l'equity peut tomber sous la marge utilisée.

## Futures

Un future a besoin de ses caractéristiques de contrat, dans les **Réglages de l'instrument** du graphique :

- **Taille du tick, valeur du point et devise.** Vides, elles sont remplies depuis les spécifications de contrats open source de [QuantConnect Lean](https://github.com/QuantConnect/Lean), intégrées à l'application et rafraîchies chaque semaine. La fenêtre nomme les champs remplis ; enregistrez pour les garder, ou saisissez les vôtres. Vos valeurs enregistrées l'emportent toujours.
- **Marge par contrat (paper)**, dans la devise de l'instrument : mettez la marge initiale de votre broker. Ou cochez **Compter le contrat à sa valeur totale** : quantité × prix × valeur du point.

Un ordre sur un future sans marge est refusé, et le graphique propose **Définir la marge** (ouvre les réglages de l'instrument) ou **Utiliser la valeur totale**. L'un comme l'autre renvoie l'ordre.

Un ticker est ramené à la racine de son contrat (`MNQZ6`, `ESZ25`, `@ES`, `ES=F`, `/ES`). Une racine cotée sur plusieurs bourses sans bourse précisée est une erreur qui nomme les candidates : **le marché n'est jamais deviné**. Un future ou une option sans valeur du point est refusé, de même que tout instrument sans devise, sauf une paire forex, dont la devise se lit dans son nom (`EURUSD` est en USD).

## Chiffres du compte

L'onglet **Compte** du panneau d'ordres et le panneau des Paramètres montrent :

| Chiffre | Calcul |
|---|---|
| **Cash** | dépôts − retraits + réalisé − commissions |
| **Résultat latent** | Σ (cours − prix moyen) × quantité × valeur du point |
| **Equity** | cash + résultat latent |
| **Réalisé** | depuis la dernière réinitialisation, avant commissions, au coût moyen |
| **Commissions** | total des frais |
| **Marge initiale** | marge par contrat des futures + valeur des autres positions ÷ levier |
| **Disponible** | equity − marge initiale, jamais sous zéro |
| **Valeur des positions** | exposition brute, longue et courte |

L'onglet se rafraîchit toutes les 5 secondes tant qu'il est ouvert.

### Autres devises

Le compte a une seule devise. Un instrument dans une autre est converti au **taux de change du jour du journal** : résultats réalisés et frais au taux de leur exécution, résultats latents et marge au taux du jour.

- Un ordre est refusé quand aucun taux n'existe : `no EUR to USD rate: add one in the journal's FX settings, or trade in USD`.
- Une position dont le taux disparaît plus tard est signalée comme exclue des totaux.

## Vers le journal

Les exécutions paper s'importent comme celles de n'importe quel broker : **Journal → Importer → Depuis un broker**, choisissez le compte paper, le book et la période, prévisualisez, importez. Voir [Importer des trades dans le journal](/fr/config/brokers#import-trades-into-the-journal). La **synchronisation automatique** fonctionne aussi.

## Bon à savoir

- Les montants sont gardés en décimaux exacts, comme un marché les enregistre : 0,1 acheté trois fois et 0,3 vendu ne laisse rien d'ouvert, et un prix doit tomber exactement sur le tick de l'instrument.
- Trésorerie, ordres et exécutions sont stockés en base et survivent aux redémarrages. Les prix en direct non : après un redémarrage, les positions sont valorisées à leur prix moyen jusqu'au prochain cours.
- Supprimer le compte supprime sa trésorerie, ses ordres et ses exécutions.
- Chaque appel d'ordre figure dans le [journal d'audit](/fr/config/trading#audit-log).
- Non modélisé : position dans la file, liquidité, exécutions partielles, intérêts, frais d'emprunt, funding, dividendes.
- Les ordres et mouvements de trésorerie paper ne sont pas exposés aux [agents IA (MCP)](/fr/config/ai-agents), et les comptes paper sont désactivés en [mode démo](/fr/guide/demo).
