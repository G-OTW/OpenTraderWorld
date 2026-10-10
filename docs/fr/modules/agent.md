# Agent

Un **assistant de chat IA** intégré : un volet de chat dans l'application qui peut aussi agir sur vos données OpenTraderWorld. Ouvrez-le depuis le module **Agent**, le raccourci étincelles de la barre du haut (à côté de la recherche), ou le [bouton flottant](#the-floating-assistant) situé dans le coin de chaque page.

L'assistant est **bring-your-own-provider** : rien n'est activé ni imposé par un éditeur tant que vous n'avez pas ajouté un fournisseur et une clé à vous.

::: tip Deux "agents IA" différents
Cette page concerne l'**assistant de chat qui vit dans l'application** et parle à un fournisseur que *vous* configurez. Ce n'est pas la même chose que la page [Agents IA (MCP)](/fr/config/ai-agents), qui traite des agents *externes* qui se connectent **à** OpenTraderWorld via le serveur MCP sortant. L'assistant de chat peut *utiliser* cette même passerelle pour atteindre vos données : voir [Outils sur vos données](#tools-over-your-data) ci-dessous.
:::

## Ajouter un fournisseur

Dans **Paramètres** (l'engrenage de la barre latérale du chat) → **Général**, ajoutez un ou plusieurs fournisseurs. Deux formats d'échange sont pris en charge :

- **Anthropic** : l'API Claude Messages.
- **Compatible OpenAI**, c'est-à-dire tout point d'entrée qui parle le format de chat OpenAI : OpenRouter, OpenAI, DeepSeek, Moonshot, Groq, Mistral, le point d'entrée de compatibilité de Gemini, un proxy local, etc.

Chaque fournisseur a sa propre **URL de base** (compatible OpenAI uniquement), **clé API** et **modèle par défaut**. La clé est en **écriture seule** : elle est chiffrée au repos avec la clé maître de l'application et n'est plus jamais affichée après l'enregistrement. Laissez le champ de la clé vide en modification pour conserver l'actuelle.

Un fournisseur peut être désactivé sans être supprimé. L'assistant n'est "prêt" que lorsqu'il dispose d'un fournisseur activé avec une clé et un modèle.

## Configurer l'assistant

Dans le même volet de paramètres, vous définissez le **prompt système**, le **fournisseur / modèle** actif, le **max tokens** et la **température**. Un champ **Paramètres avancés (JSON)** transmet tel quel au fournisseur tout champ de requête supplémentaire. Mettez une valeur à `null` pour *retirer* une clé que l'application enverrait sinon (p. ex. `max_completion_tokens` pour les modèles OpenAI récents, ou supprimer `stream_options`).

## Chat

- Les réponses sont **diffusées en direct** et rendues en Markdown. Les modèles qui exposent leur raisonnement ont un pli **Réflexion** optionnel.
- Les conversations sont enregistrées dans la **barre latérale** : nouvelle, sélectionner, renommer, supprimer, et **export Markdown** en un clic.
- **Déverser dans Editor** transforme une conversation en page Editor : choisissez un dossier (ou créez-en un) et un nom de fichier, puis **Enregistrer** pour rester sur l'agent ou **Enregistrer et ouvrir**. Chaque message garde sa date et son auteur (Vous, ou la persona et le modèle), le raisonnement figure dans une citation au-dessus de la réponse, et **Inclure les détails** ajoute les appels d'outils et les comptes de tokens.
- Vous pouvez **arrêter** une exécution en cours de diffusion.
- L'en-tête du chat affiche un **compteur de tokens** cumulé (entrée + sortie) pour la conversation, pour voir ce que coûte un fil.
- Un bouton **mode large** supprime la limite de largeur de lecture. Le fil est dimensionné pour la prose, ce qui n'est pas la bonne forme pour les tableaux que produit l'assistant : un registre d'essais ou une ventilation de statistiques demande de la place. Le choix est conservé par navigateur.
- Les échecs du fournisseur se lisent comme une phrase simple dans une bannière fermable : une clé rejetée, une limite de débit (avec le retry-after du fournisseur quand il en envoie un), un mauvais modèle ou une mauvaise URL de base, un refus du filtre de contenu, ou une réponse coupée à la limite de tokens.

### Limites du chat

Un chat peut être plafonné en **tokens de sortie**, en **dollars**, ou les deux. Définissez les valeurs par défaut dans **Paramètres → Général → Limites des nouveaux chats** ; chaque nouveau chat démarre avec elles, et les modifier plus tard laisse les chats existants intacts. Laissez un champ vide pour ne pas limiter.

La fine barre à côté du bouton Prompt Store dans la zone de message se remplit de bas en haut à mesure que le chat dépense. Survolez-la pour voir les chiffres, cliquez dessus pour changer les limites de ce chat. Quand une limite est atteinte, l'exécution s'arrête avant sa prochaine étape payante et le chat n'accepte plus de message jusqu'à ce que vous releviez ou retiriez la limite, ce que vous pouvez faire depuis la même barre. Envoyer à la limite affiche un avis avec deux raccourcis : **Modifier la limite** ouvre cet éditeur, **Nouveau chat** ouvre un chat neuf avec la même persona et conserve ce que vous aviez tapé.

La limite en dollars exige un fournisseur qui renvoie le prix avec chaque réponse (OpenRouter le fait, Anthropic non). Sans cela, la dépense s'affiche comme "aucun prix renvoyé" et seule la limite de tokens s'applique.

### Changer de fournisseur ou de modèle par chat

L'en-tête du chat affiche le **fournisseur · modèle** actif. L'ouvrir donne un **écran de sélection complet** : les fournisseurs d'un côté, et la **liste de modèles en direct** du fournisseur sélectionné de l'autre, avec recherche, interrogée côté serveur pour que votre clé n'atteigne jamais le navigateur. La saisie libre fonctionne toujours pour les proxys qui n'exposent pas de liste. Rien ne change tant que vous ne confirmez pas avec **Utiliser ce modèle**, donc parcourir la liste ne coûte rien, et un seul bouton remet la conversation sur le défaut hérité.

Le choix appartient à **cette conversation**, pas à l'assistant : un modèle rapide et bon marché peut relire votre journal dans un onglet pendant que le modèle de raisonnement le plus fort débat d'un backtest dans un autre. Une conversation qui n'a pas fait de choix propre hérite de celui de la persona, puis de ce que vous avez défini dans **Paramètres → Général**. Un point sur le sélecteur marque ceux qui tournent sur quelque chose de propre, et un clic les remet sur le réglage hérité. Changer de fournisseur efface aussi l'identifiant du modèle, puisqu'un nom de modèle n'a de sens que pour l'éditeur dont il vient.

### Envoyer un prompt enregistré

Le composeur peut puiser dans votre [Prompt Store](/fr/modules/productivity#prompt-store) au lieu de retaper un prompt que vous conservez. Le sélecteur liste vos prompts avec une recherche sur le nom, les tags et le corps, prévisualise le prompt sélectionné en entier, et le dépose dans le composeur avec **Insérer le prompt** (ou un double-clic sur la ligne), où vous pouvez encore le modifier avant l'envoi.

## L'assistant flottant {#the-floating-assistant}

Un bouton dans le **coin inférieur droit de chaque page** ouvre un chat compact par-dessus vos conversations existantes, sans quitter ce que vous faisiez. C'est le même assistant, pas un parallèle : mêmes conversations, personas, fournisseurs et outils, donc un fil commencé dans le coin se retrouve ensuite sur la page Agent et inversement. La persona, le modèle et les outils tiennent sur une ligne sous le titre, et les sélecteurs de modèle et de prompt s'ouvrent comme une vue au-dessus du fil plutôt que comme une boîte de dialogue.

Il sait aussi **sur quelle page vous êtes**. Chaque message porte le module courant, et quand le token de la conversation accorde ce module sa liste de points d'entrée est chargée d'emblée dans le prompt, donc une demande faite depuis Historical Data ne dépense pas son premier tour d'outils à chercher où regarder. Tout le reste reste à une recherche de distance, et une page dont l'assistant n'a que faire (Paramètres, le dashboard) n'envoie rien du tout.

## Mémoire et compétences

Deux onglets dans les paramètres permettent à l'assistant de porter des connaissances d'une conversation à l'autre :

- **Mémoire** : petits faits durables (une préférence, un détail stable) qui persistent entre les chats. Seul l'**index** (slug + description d'une ligne) voyage dans le prompt ; le contenu complet est récupéré à la demande. Vous parcourez, modifiez et supprimez chaque mémoire vous-même, rien n'est caché. Chaque mémoire enregistre **quelle persona l'a écrite**, affiché à la fois dans le gestionnaire et dans l'index que lit l'assistant : la mémoire est un stockage partagé, donc une contrainte écrite par le Day Trader se lirait sinon pour l'Analyste comme la sienne. L'assistant peut aussi élaguer lui-même des mémoires quand le stockage se remplit, et ne peut pas écraser silencieusement une mémoire que vous avez écrite à la main.
- **Compétences** : des jeux d'instructions Markdown réutilisables que vous définissez. Le **nom + la description** d'une compétence sont toujours dans le contexte ; l'assistant charge le corps complet à la demande quand une tâche l'exige. Activez/désactivez chaque compétence individuellement.

Les longues conversations reçoivent aussi un **résumé glissant** : quand un chat devient gros, les anciens tours sont compressés en un résumé courant pour que le fil reste économique, en ne gardant que les messages les plus récents tels quels.

## Personas

Une **persona** est une version de l'assistant façonnée par un rôle : un prompt système avec une posture et une frontière de refus explicite, plus une **étagère de compétences** choisie. Cinq sont fournies (**Quant**, **Portfolio Manager**, **Day Trader**, **Researcher**, **Financial Analyst**) et vous en choisissez une à l'ouverture d'une conversation, depuis le sélecteur de persona dans l'en-tête du chat.

L'idée est l'étroitesse. Un assistant générique avec deux cents points d'entrée est moins bon pour chaque tâche qu'un qui en connaît quelques-uns à fond et refuse le reste. Le Quant ne rapportera pas un backtest sans le nombre d'essais et un chiffre hors échantillon ; le Day Trader ne nommera pas d'entrée ; l'Analyste rapporte les fondamentaux qu'il *n'a pas pu* obtenir plutôt que de les inventer.

### Ce qu'une persona n'est pas

**Une persona n'est pas un jeu de permissions.** Ce que l'assistant peut atteindre est le token MCP de la conversation : limité par module, défini par vous, identique quelle que soit la persona qui parle. Changer de persona resserre la *posture et l'étagère*, jamais l'accès aux données. Pour changer ce qu'il peut toucher, changez le token.

### Changer en cours de conversation

Vous pouvez changer de persona en plein fil. Cela prend effet **à partir du message suivant**, et un marqueur arrive dans la transcription pour consigner la passation : les tours au-dessus ont été produits par la persona précédente et lui restent attribués.

### Modifier les vôtres

**Paramètres → Personas** liste chaque persona avec l'étagère qu'elle recevra réellement. De là vous pouvez :

- en **créer** une de zéro, ou **dupliquer** une fournie et réécrire la copie ;
- modifier le **prompt**, l'**étagère**, et si elle **approuve automatiquement les écritures** ;
- **réinitialiser** une intégrée à son état d'origine (vos modifications sont perdues, rien d'autre n'est touché) ;
- **supprimer** une que vous avez créée. Ses conversations sont **conservées** : elles passent à l'assistant par défaut, et chaque transcription reçoit une note qui le dit. Les intégrées ne peuvent pas être supprimées : l'application les recrée au prochain redémarrage, donc une suppression n'aurait que l'apparence de fonctionner.

Les compétences ne sont **pas** créées ici. Il y a un seul catalogue, géré dans l'onglet Compétences, et les personas y puisent. Modifier le corps d'une compétence la change donc pour toutes les personas qui la détiennent, et la liste des compétences montre combien, donc la modification n'est jamais aveugle.

### Export et import

Toute persona s'exporte en **fichier JSON avec les corps de compétences intégrés**, donc un seul fichier la reproduit sur une autre machine. Vous pouvez aussi exporter une compétence seule, ou toute l'étagère d'un coup. L'import accepte l'une ou l'autre forme ; un nom existant est ignoré plutôt qu'écrasé. Il n'y a pas de barrière de revue : c'est votre machine, et ce que vous chargez dans votre propre assistant est votre décision.

La gestion des personas et des compétences est volontairement **absente du catalogue MCP** : aucun agent, ni aucun contenu qu'un agent lit, ne peut modifier une persona ou élargir une étagère.

## Confirmation d'écriture

Quand l'assistant veut modifier vos données, l'exécution se **met en pause** et vous montre l'appel exact (méthode, chemin et corps) avec Approuver et Refuser. Rien n'est écrit tant que vous n'avez pas répondu, et un refus est rapporté au modèle comme un refus plutôt que comme une erreur à contourner. Si vous vous éloignez, l'attente expire et l'écriture n'a pas lieu.

Une persona peut être réglée sur **approuver automatiquement les écritures**, ce qui saute l'invite pour les modifications ordinaires. **Les suppressions demandent toujours**, quoi que dise ce réglage : cocher la case était une décision sur les écritures courantes, pas la permission d'effacer un journal.

Les points d'entrée qui ne font que *calculer* (un backtest, un ratio de Sharpe, un Monte-Carlo) n'envoient pas d'invite. Ils ne changent rien dont vous sentiriez l'absence, et une boîte de confirmation à chaque calcul est la façon dont on apprend à cliquer sur Approuver sans lire.

## Outils sur vos données {#tools-over-your-data}

Rattachez un **token MCP** à une conversation et l'assistant peut lire et modifier vos modules via la [même passerelle interne](/fr/config/ai-agents) que celle des clients MCP externes. Les **niveaux de permission par module** du token (Lecture / Lecture+écriture / Complet, définis dans **Paramètres → MCP**) s'appliquent **directement** : le token *est* l'enveloppe de permissions ; il n'y a pas de seconde barrière côté agent. Les opérations sur les paramètres, les secrets, le réseau et l'effacement de données ne sont jamais exposées, et il n'y a ni shell ni accès au système de fichiers par construction.

Les appels d'outils apparaissent en ligne sous forme de **pastilles repliables** montrant les arguments et le résultat. Une exécution est bornée à 15 tours d'outils, et chaque conversation porte un **budget de simulation**, car les backtests et balayages sont la seule chose qu'un assistant peut dépenser sans limite, donc une fois le budget épuisé il reçoit l'ordre d'arrêter de chercher et de rapporter ce qu'il a, y compris le nombre d'essais effectués.

### Écrire votre propre compétence

Une compétence est une procédure, pas un manuel. La forme qui fonctionne :

- une **description** qui dit *quand* y recourir, puisque cette ligne est la clé de récupération et voyage dans chaque prompt, donc "À utiliser quand l'utilisateur propose une stratégie" vaut mieux que "À propos des stratégies" ;
- un **corps** qui nomme les points d'entrée exacts, étape par étape, avec les façons précises dont la tâche tourne mal dans cette application ;
- une étape de **vérification** : comment prouver le résultat avant de le rapporter ;
- une **forme de rapport** : ce que la réponse doit contenir.

Gardez le corps court. Il arrive en entier dans la fenêtre de contexte quand il est chargé, donc un long corps évince la tâche qu'il devait aider. L'éditeur vous avertit au-delà d'environ deux mille mots.

### Outils par conversation

Chaque conversation porte **son propre** token MCP (le token défini dans les paramètres n'est que le défaut des nouvelles conversations), modifiable depuis un **menu déroulant d'outils** dans l'en-tête du chat. Deux conversations peuvent tourner avec des portées de données différentes côte à côte. Le menu déroulant :

- a une **zone de recherche** pour filtrer tokens et serveurs externes par nom ;
- signale quand le token sélectionné accorde **écriture/suppression** ;
- propose des actions en ligne pour **ajouter un serveur MCP** et des raccourcis vers **Paramètres → MCP** (créer/gérer les tokens) et le **MCP store**.

## MCP store : connecter des plateformes externes

**Agent → Gérer les serveurs** est une section pleine page pour ajouter des serveurs MCP distants afin que l'assistant atteigne des plateformes extérieures :

- un **catalogue organisé** de serveurs connus (DeepWiki, Context7, GitHub, Hugging Face, apportez votre propre clé), plus des **serveurs personnalisés** par URL ;
- **Streamable-HTTP uniquement** : rien ne s'exécute jamais en local ;
- les valeurs d'authentification sont **chiffrées au repos et en écriture seule** ;
- un bouton **Tester** se connecte et liste les outils du serveur ;
- activez un serveur par conversation depuis le menu déroulant d'outils.

Les outils externes sont **préfixés par un espace de noms** (p. ex. `deepwiki__ask_question`) et étiquetés avec leur serveur, les appels sont limités dans le temps, et un serveur injoignable **dégrade en avertissement** au lieu de bloquer le chat.

::: warning Le contenu externe n'est pas fiable
Un serveur MCP externe voit votre conversation, et ce qu'il renvoie est du contenu tiers. Combiner un serveur externe avec un token qui accorde un **accès en écriture** à vos données signifie qu'un contenu injecté pourrait tenter de déclencher des modifications, et le menu déroulant d'outils vous avertit quand cette combinaison est active. N'ajoutez que des serveurs de confiance, et gardez un œil sur les pastilles d'appels d'outils.
:::
