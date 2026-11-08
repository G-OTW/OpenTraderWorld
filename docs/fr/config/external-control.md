# Contrôle externe (chat)

Pilotez OpenTraderWorld depuis **Telegram, Slack ou Discord**. Un message que vous envoyez à votre bot devient une exécution de l'une de vos [personas d'agent](/fr/modules/agent), avec réponse dans le chat, exactement avec l'accès que permet le token choisi.

**C'est désactivé par défaut**, et cela n'ajoute aucun système de permissions propre : le plafond est un [token MCP](/fr/config/ai-agents), le même que celui de l'assistant intégré.

## Rien de nouveau n'écoute sur votre machine

Les trois transports **appellent vers l'extérieur** : long polling Telegram, Socket Mode Slack, gateway Discord. Il n'y a aucune URL publique à publier, aucun port à ouvrir et aucune route entrante à attaquer, donc cela fonctionne sans changement sur l'installation par défaut en localhost uniquement et derrière un NAT.

Le canal que vous utilisez déjà pour les notifications ne peut qu'**envoyer** (l'URL d'un webhook Slack ou Discord est en écriture seule). Recevoir exige un vrai bot, donc une liaison détient son propre identifiant :

| Plateforme | Identifiant à coller | Sur la plateforme |
|---|---|---|
| **Telegram** | le token du bot BotFather | rien d'autre |
| **Slack** | **les deux** tokens, `xapp-…` et `xoxb-…`, séparés par un espace ou un retour à la ligne | Socket Mode activé, événement `message.im`, scope `chat:write` |
| **Discord** | le token du bot | l'intent **Direct Messages** (l'intent privilégié de contenu des messages n'est pas nécessaire pour les DM) |

## En mettre un en place

1. **Paramètres → Notifications** : créez le canal si vous n'en avez pas. C'est le chemin de réponse.
2. **Paramètres → MCP** : créez ou modifiez un token, définissez ses niveaux par module, et cochez **Autoriser l'accès externe**.
3. **Paramètres → Contrôle externe** : **Nouvelle liaison**, choisissez le canal, la persona et ce token, collez l'identifiant du bot (ou branchez-le depuis le [Coffre](/fr/config/settings#vault)), définissez éventuellement le fournisseur et le modèle sur lesquels démarrent les nouveaux chats, activez la liaison.
4. Activez l'interrupteur de la section. La liaison affiche **Connecté** en quelques secondes.
5. **Appairer** : cliquez sur *Appairer*, puis envoyez le code à 6 chiffres à votre bot **depuis le compte qui doit avoir le droit de piloter**. Il ne fonctionne qu'une fois, et dure aussi longtemps que l'indique *Durée du code d'appairage* (une heure par défaut, de 5 minutes à un jour).

Tant que personne n'est appairé, le bot ne répond à personne, vous compris.

## Quel modèle répond

Chaque chat porte son propre fournisseur et modèle, exactement comme une conversation dans l'application. La liaison définit le **défaut sur lequel démarre un nouveau chat** : un échange sur téléphone mérite souvent un modèle moins cher et plus rapide que celui que la même persona utilise dans le navigateur. Laissez vide et un chat hérite de celui de la persona.

Changez le défaut dans le formulaire de la liaison. Changez un chat depuis le chat lui-même :

- `/provider` liste les fournisseurs configurés, `/provider 2` ou `/provider openrouter` y bascule ce chat (ce qui réinitialise le modèle au défaut de ce fournisseur, puisqu'un identifiant de modèle appartient à un seul éditeur).
- `/model` liste les modèles de ce fournisseur, `/model 3` choisit par position et `/model haiku` choisit par texte quand exactement un identifiant correspond.

Le choix reste sur ce chat et ne déplace rien d'autre. `/new` abandonne la conversation, donc la suivante repart sur le défaut de la liaison.

## Droits

Le token décide de tout ce qu'une réponse peut toucher : *aucun accès*, *lecture*, *lecture + écriture*, *complet* par module, exactement comme sur la [page MCP](/fr/config/ai-agents#security-model). Le drapeau `external` n'élargit rien, il dit seulement que cette enveloppe peut être atteinte depuis l'extérieur.

- **Un token par liaison, une liaison par canal.** Révoquer un token arrête cette liaison et rien d'autre, et la piste d'audit dit toujours par où un appel est arrivé.
- **Utilisez un token dédié**, et commencez en lecture seule. Vous pourrez l'élargir plus tard sans ré-appairer.
- Un token **expiré** ou qui perd le drapeau arrête la liaison au message suivant, pas au prochain redémarrage.

## Qui peut piloter

Un chat est un lieu, pas une identité, donc l'autorité est liée à l'**identifiant d'expéditeur** de la plateforme :

- Seul un expéditeur appairé reçoit une réponse. Tout autre est **ignoré sans réponse**, ce qui est voulu : un refus dit à un inconnu que le bot existe.
- Les expéditeurs appairés sont listés sur la liaison. Cliquez sur l'un pour le retirer.
- **Messages privés uniquement.** Un groupe laisserait plusieurs personnes écrire dans le prompt d'un agent qui peut détenir une autorisation d'écriture.

## Les écritures demandent toujours

Chaque écriture vous est soumise dans le chat avec la méthode, le chemin et le corps exacts, et attend un mot. Seuls `yes`, `y`, `ok`, `okay`, `approve`, `oui` ou `go` l'approuvent ; tout autre mot, ou le silence, la refuse et l'agent est informé du refus.

Le réglage **approuver automatiquement les écritures** de la persona ne se transmet pas. Il a été coché dans une session authentifiée de l'application ; il ne vous suit pas sur un téléphone.

## Ce qu'un message traverse

Dans l'ordre, tout :

1. l'interrupteur global dans **Paramètres → Contrôle externe**
2. la liaison activée
3. le token qui porte toujours `external`, et non expiré
4. un chat privé
5. l'expéditeur présent sur la liste d'autorisation
6. une limite de débit de 12 messages par minute par liaison

Ensuite l'exécution elle-même est soumise à la liste d'autorisation du catalogue MCP : les routes de compte, de réseau, de secrets, de stockage de fichiers et d'effacement de données sont inaccessibles quoi que dise le token, tout comme la gestion des liaisons. Aucun agent ne peut créer une liaison, générer un code d'appairage ni élargir sa propre portée.

## Notes de sécurité

- **Le token du bot est l'accès.** Quiconque le détient peut parler à votre instance au niveau de cette liaison, avec la liste d'autorisation des expéditeurs toujours en travers. Gardez-le dans le [Coffre](/fr/config/settings#vault) et commencez en lecture seule.
- **L'injection de prompt est le vrai risque**, pas le transport. Le contenu que l'agent lit (mail, flux, webhooks entrants) peut porter des instructions. Ce qui la contient est la même chose que dans l'application : la liste d'autorisation du catalogue, les niveaux du token et la confirmation d'écriture ci-dessus.
- Les **codes d'appairage** font six chiffres, sont à usage unique et limités dans le temps, et n'existent qu'entre le clic sur *Appairer* et leur utilisation. Raccourcissez la fenêtre dans l'en-tête de la section si un code doit rester affiché à l'écran.
- Les identifiants des bots sont scellés au repos et jamais affichés, y compris dans les messages d'erreur.
- Désactivé en bloc en [mode démo](/fr/guide/demo).

## Limites

- **Texte uniquement.** Pas de graphiques ni de fichiers ; un graphique vit toujours dans l'application.
- **Pas de streaming token par token.** Les plateformes de chat n'offrent que des modifications de message, et les limitent en débit, donc la réponse arrive par blocs d'environ une seconde et demie et est découpée au plafond de la plateforme (4096, 3000 et 2000 caractères).
- Commandes : `/new` démarre une conversation neuve pour ce chat, `/provider` et `/model` listent et changent ce avec quoi ce chat répond, `/whoami` montre l'identifiant sous lequel vous êtes appairé, `/help`.
- Un redémarrage abandonne une confirmation d'écriture en attente. Rien ne s'exécute sans confirmation, on vous redemande simplement.
