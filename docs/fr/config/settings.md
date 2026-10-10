# Référence des paramètres

Tout ce qui se trouve sous l'entrée **Paramètres** du sélecteur de modules, section par section.

## Compte

Changez votre nom d'utilisateur ou votre mot de passe. Votre mot de passe actuel est requis pour enregistrer les modifications, et changer le mot de passe vous **déconnecte de toutes les sessions**.

Un nouveau mot de passe doit comporter au moins 12 caractères et est refusé s'il figure dans les listes publiques de fuites. Voir [Règles de mot de passe](/fr/config/security#password).

## Sécurité

Authentification à deux facteurs, navigateurs actuellement connectés à votre compte, et durée maximale d'une requête. Détaillé dans [Sécurité du compte](/fr/config/security).

## Valeurs par défaut

- **Langue** : s'applique immédiatement à toute l'application (en, fr, de, es, it, pt, zh).
- **Devise par défaut** et **fuseau horaire** : les valeurs de départ que les modules utilisent pour les nouveaux éléments et l'affichage.

## Apparence

La **couleur d'accent** de l'application, celle des boutons principaux, des états actifs, des liens et des mises en évidence de graphiques. Choisissez une pastille prédéfinie ou n'importe quelle couleur dans le sélecteur ; elle s'applique en direct dans toute l'application et est enregistrée pour chaque session. *Réinitialiser* la remet à la valeur par défaut du thème.

## Réseau {#network}

Qui peut joindre l'application : localhost, réseau local, LAN + HTTPS ou public. Détaillé dans [Réseau et accès à distance](/fr/config/network).

## Coffre {#vault}

Un endroit unique pour les clés API et secrets que l'application utilise en votre nom, au lieu de coller la même clé dans chaque module qui en a besoin.

Un **coffre** représente un service externe (p. ex. *Binance*) et contient des **clés** nommées : `apikey`, `secretkey`, etc. Créez autant de coffres et de clés que nécessaire ; les modules **branchent ensuite une clé par référence** via un sélecteur partagé, partout où un secret est demandé (connectors de fournisseurs, identifiants de flux…).

- **Valeurs en écriture seule.** Un secret est scellé à l'enregistrement et ne peut plus jamais être consulté, seulement remplacé ou supprimé. Les *noms* des clés restent visibles. Tout est chiffré au repos avec la clé maître de l'application.
- **Débrancher avant de supprimer.** Supprimer un coffre ou une clé encore branché à un module est bloqué ; retirez d'abord la référence là-bas. Chaque coffre indique combien de connexions l'utilisent.
- **Le suivi des requêtes** est optionnel et **par coffre, pas par clé** : toutes les clés d'un coffre comptent dans le même compteur. La limite est purement informative (observer et afficher) ; rien n'est jamais bridé. Il alimente la même vue que [Débit API](#api-rate).

Les secrets des flux d'actualités acceptent aussi des marqueurs en ligne <code v-pre>{{vault.item}}</code>, résolus par le planificateur au moment de l'interrogation. Voir [Actualités](/fr/modules/news-research#news).

## Modules

Installez et détachez des modules. Tout est livré avec l'application : installer ne fait que rendre un module disponible dans le sélecteur et sur le dashboard ; rien n'est téléchargé. Détacher le masque et le rend inaccessible ; cochez *supprimer aussi les données* pour effacer ses données stockées (définitif).

## Gérer les données

Utilisation du stockage par module (tables, lignes, taille) avec le total de la base, et une action **Effacer** pour supprimer définitivement les données d'un module (saisissez son nom pour confirmer). L'effacement est irréversible.

## Versions {#versioning}

Deux interrupteurs : **Fichiers de l'éditeur** et **Stratégies**, chacun avec le nombre de versions stockées et leur taille. En activer un avertit que chaque version est une copie complète et que la base grossit à chaque fois (les images et vidéos ne sont jamais dupliquées). En désactiver un demande s'il faut conserver les versions (masquées jusqu'à la réactivation) ou toutes les supprimer. Une fois un domaine activé, le versionnage s'active par fichier ou par stratégie depuis son menu d'historique : voir [Éditeur](/fr/modules/productivity#editor) et [Backtest](/fr/modules/market-data#strategies-and-custom-indicators).

## Sauvegarde et restauration

Deux onglets, chacun avec un côté **Sauvegarde** et un côté **Restauration** :

- **Complète** : commandes `pg_dump` et `psql` prêtes à copier pour votre déploiement, y compris les variantes chiffrées, plus l'état de la sauvegarde automatique sur les instances qui en exécutent une.
- **Partielle** : choisissez les modules voulus, téléchargez-les dans un seul zip, et rechargez ce zip ici ou sur une autre instance. Les deux côtés sont comptés d'abord : ce que vous prenez, par module et par table, et ce que contient un fichier que vous chargez face à ce qui est déjà là.

Voir [Sauvegarde et restauration](/fr/guide/backup-restore).

## Mettre à jour l'app

Affiche la version actuelle, vérifie sur GitHub s'il en existe une plus récente, et liste les commandes de mise à jour à exécuter sur l'hôte. Voir [Mise à jour](/fr/guide/updating).

## Journaux

Le stockage de logs propre à l'application, avec recherche par message/cible. Le **niveau de capture** définit la gravité minimale écrite dans le stockage (effet immédiat). Les niveaux plus bas capturent plus de détails et utilisent plus d'espace. Vous pouvez effacer les logs stockés ici.

## Débit API {#api-rate}

Un dashboard des appels sortants vers les fournisseurs de données externes (données de marché, change, cotations, flux), comptés par jour UTC : nombre de requêtes par fournisseur, erreurs, réponses de limitation de débit, limites publiées quand elles sont connues, et une liste des dernières limitations atteintes. Il existe pour que vous voyiez à quel point vous approchez des limites des offres gratuites d'un fournisseur.

**Cette page ne bride jamais rien** : elle observe seulement. Le seul endroit où une limite est réellement appliquée est la [limite de requêtes propre à un connector](/fr/config/connectors#request-limits) pour les récupérations à la demande du graphique ; partout ailleurs une limite informe et avertit, et c'est le fournisseur qui reste celui qui dit non.

## Data connectors

La liste partagée des comptes de fournisseurs de données de marché utilisés par Historical Data, Visualization, Watchlists et le Journal de trading : identifiants, limites de requêtes, et quels modules peuvent utiliser chacun. Détaillé dans [Data connectors](/fr/config/connectors). Le même écran est aussi disponible seul à **/connectors**, et depuis le bouton de connector dans chaque module de données.

## Brokers {#brokers}

La liste partagée des **comptes broker en lecture seule** utilisés par le Journal de trading, Portfolios, Visualization et le Calculateur fiscal : identifiants, réglages par broker, et quels modules peuvent utiliser chacun. Détaillé dans [Comptes broker](/fr/config/brokers). Rien ici ne peut passer, modifier ou annuler un ordre.

## Notifications {#notifications}

La liste partagée des **canaux de notification** : où l'application a le droit de pousser, créés une fois et réutilisés par chaque module qui notifie.

Un canal est une destination **qui vous appartient**. Chacun contient un secret, saisi ici ou branché depuis le [Coffre](#vault), scellé à l'enregistrement et jamais réaffiché.

| Canal | Ce que vous apportez | Secret | Autres champs |
|---|---|---|---|
| **E-mail** | votre propre serveur SMTP | mot de passe | hôte, port (587 STARTTLS, 465 TLS), expéditeur, destinataire, nom d'utilisateur |
| **Telegram** | un bot BotFather | token du bot | identifiant du chat |
| **Slack** | un Incoming Webhook | l'URL du webhook | aucun |
| **Discord** | un Webhook de salon | l'URL du webhook | aucun |

Les quatre sont gratuits pour l'hôte : vous apportez le compte, l'application n'impose aucune inscription. Certains champs **non secrets** acceptent aussi un élément du coffre, l'**identifiant du chat** Telegram par exemple, afin qu'un canal puisse être configuré sans que cet identifiant soit en clair dans la configuration.

Les modules auxquels un canal peut être accordé :

| Module | Ce qu'il pousse |
|---|---|
| **RemindMe** | un rappel déclenché |
| **Watchlists** | une alerte de prix |
| **Mailbox** | un compte mail qui demande de l'attention |
| **Webhooks** | une charge entrante redirigée vers un module |
| **Historical Data** | une longue pause, et la fin d'un lot de téléchargements |
| **Visualization** | une alerte de graphique déclenchée |
| **Journal** | l'enrichissement par données de marché d'un nouveau trade |
| **Backtest** | une exécution de paper trading, ou son résumé groupé |
| **Portfolio Tracker** | accordable à l'avance de ce qu'il enverra ; il ne pousse rien aujourd'hui |
| **Automator** | tout ce qu'un bloc `notify` envoie |

- **Autorisations, par module.** Chaque canal nomme les modules autorisés à lui envoyer, ou *tous*. La vérification s'exécute côté serveur : un module à qui aucun canal n'a été accordé ne peut pas l'atteindre, et le secret de ce canal n'est même jamais déchiffré pour lui.
- **Un interrupteur par canal.** Désactiver un canal le fait taire partout sans le supprimer ni supprimer ses identifiants.
- **Envoi de test** avant de vous y fier.

Le même écran s'ouvre depuis chaque module qui notifie, afin qu'un canal puisse être ajouté sur le moment sans quitter la page où vous êtes. Il est volontairement absent du catalogue [MCP](#mcp) : aucun agent ne peut créer un canal ni élargir une autorisation.

## MCP {#mcp}

Laissez des agents IA utiliser l'application via une passerelle contrôlée. Détaillé dans [Agents IA (MCP)](/fr/config/ai-agents).

## Contrôle externe {#external-control}

Pilotez l'application depuis un canal de chat (Telegram, Slack, Discord). Détaillé dans [Contrôle externe (chat)](/fr/config/external-control).

## Voix {#voice}

Commandes push-to-talk et dictée : le moteur de reconnaissance vocale, les deux raccourcis et vos commandes vocales. Détaillé dans [Commande vocale](/fr/config/voice). Le microphone ne fonctionne qu'en HTTPS ou sur localhost.

## Crédits

Les sources de données et projets amont que chaque module peut utiliser, y compris les fournisseurs que vous n'avez pas configurés.

## À propos

Version, liens du projet et boutons de partage.
