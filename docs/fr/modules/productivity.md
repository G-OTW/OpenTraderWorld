# Notes et organisation

Les modules du quotidien : documents, tâches, objectifs, calendrier, rappels, plus quelques-uns conçus pour la discipline d'un trader.

## Editor {#editor}

Un éditeur de documents enrichi dans le style de Notion. Les documents vivent dans une arborescence de dossiers ; tapez `/` pour le menu de blocs.

- **Blocs** : titres, listes, listes de tâches, citations, blocs de code, séparateurs, liens, images (téléversées ou par URL), couleur du texte, surlignage, taille de police, largeur normale ou pleine. Enregistrement automatique.
- **Bases de données** : un type de document avec des colonnes typées (texte, sélection, URL…), affichable en **table**, **kanban** (groupé par une colonne de sélection) ou **galerie** (avec une colonne d'image de couverture). Glissez pour réordonner lignes et colonnes.
- **Soumettre pour publication** : envoie un document dans la file de revue des [Community Docs](/fr/modules/news-research#community-docs), mise en forme conservée, avec langue, catégories et mention d'auteur optionnelle.
- **Versions** (optionnel) : activez le versionnage dans [Paramètres → Versions](/fr/config/settings#versioning), puis par page ou base de données depuis son menu d'historique. **Enregistrer une version** stocke un instantané avec sa date et une note optionnelle ; ouvrez-en une pour la voir en lecture seule, la restaurer (l'état restauré est enregistré comme une nouvelle version, annotée avec la date de la version restaurée) ou la supprimer. L'historique s'ouvre dans une fenêtre avec une recherche sur les notes et les dates ; la version correspondant au fichier actuel est marquée. Les notes sont plafonnées à 500 caractères. Les images et vidéos ne sont pas copiées : une version pointe vers les mêmes fichiers téléversés. Désactiver le versionnage pour un fichier demande s'il faut conserver ou supprimer ses versions. Supprimer un fichier supprime aussi ses versions, après confirmation. Les agents avec accès en écriture à Editor peuvent enregistrer, restaurer et supprimer des versions via [MCP](/fr/config/ai-agents).

## ToDo {#todos}

Une liste de tâches qui reste discrète : des tâches avec échéance, heure, catégorie et notes. Filtrez par en attente/terminées/en retard, triez par échéance ; les indicateurs en retard/aujourd'hui/bientôt font le rappel. Un widget du dashboard montre ce qui est ouvert.

## Goals {#goals}

Des objectifs avec des **métriques mesurables**. Donnez à chaque objectif une échéance, une catégorie, et une ou plusieurs métriques avec une valeur actuelle, une cible et des points. Incrémentez-les au fil de votre progression, et l'accomplissement de l'objectif suit les points. Filtrez ouverts/atteints/en retard ; glissez pour ordonner.

## Calendar {#calendar}

Un calendrier personnel (année/mois/semaine/jour) pour des événements avec catégorie, couleur, lieu et notes. Son atout est les **superpositions** : il peut aussi afficher vos **Reminders**, vos **ToDos avec échéance** et les **échéances de Goals**, chacun activable : un seul endroit pour voir la semaine. Créer un événement peut aussi créer un rappel synchronisé à l'heure de début.

## RemindMe {#remindme}

Des rappels, ponctuels ou récurrents (avec date de début, date de fin ou nombre maximal), qui se déclenchent en **notifications dans l'application** avec une boîte de notifications.

- **Rappels liés** : rattachez un rappel à un élément d'un autre module (un objectif, la facturation d'un abonnement, une revue de journal…) et il renvoie directement vers lui. La plupart des modules ont un bouton *Ajouter un rappel* qui pré-remplit cela.
- **Canaux** : livrez aussi par **e-mail, Telegram, Slack ou Discord**, choisis parmi les [canaux de notification](/fr/config/settings#notifications) partagés. Un rappel liste les canaux accordés à RemindMe ; les identifiants et les autorisations vivent dans les Paramètres, une fois pour toute l'application.

## Webhooks {#webhooks}

Donnez à n'importe quel service externe une URL privée pour **POSTer des alertes dans OpenTraderWorld** : plateformes de graphiques et d'alertes, notifications de broker, moniteurs de disponibilité, scripts, tout ce qui peut émettre une requête HTTP. La charge est reçue et routée vers un module.

- **URL privée, sans en-têtes** : chaque point d'entrée porte un **token de 256 bits dans le chemin de l'URL** (`/api/hooks/<token>`), car beaucoup d'émetteurs d'alertes ne peuvent pas définir d'en-tête `Authorization`. Les tokens sont stockés **hachés** et affichés **une seule fois** à la création ; les recherches échouées sont limitées.
- **Charges tolérantes** : envoyez du texte brut ou du JSON ; l'analyseur accepte des noms de champs souples, donc la plupart des émetteurs fonctionnent sans mise en forme spéciale.
- **Routage** : chaque point d'entrée redirige sa charge vers un module cible. La cible de la v1 est **[RemindMe](#remindme)** : une charge entrante devient une notification dans l'application, poussée ensuite vers vos canaux activés (e-mail/Telegram/Slack/Discord).
- **Journal de livraison** : les livraisons les plus récentes par point d'entrée sont conservées pour confirmer qu'un émetteur vous atteint et voir ce qu'il a envoyé.

Gérez les points d'entrée à **/webhooks**.

::: warning L'émetteur doit pouvoir vous joindre
Un webhook n'est utile que si le service émetteur peut ouvrir une connexion vers votre hôte. En mode réseau `local` (et LAN simple) rien d'extérieur ne le peut, et la page vous avertit quand le mode actuel n'est pas joignable depuis Internet. Changez le mode dans [Paramètres → Réseau](/fr/config/network), ou pointez un tunnel (p. ex. Cloudflare Tunnel) vers l'hôte et gardez l'application privée par ailleurs.
:::

## Trading Routines {#routines}

Des **checklists de séance** récurrentes dues les jours de semaine que vous choisissez : préparation pré-marché, discipline en séance, revue post-marché. Cochez les éléments jour par jour, parcourez les jours passés, et surveillez la **bande de régularité sur 14 jours** pour voir si vous tenez vraiment votre processus. Des checklists de départ sont incluses.

## Time Tracker {#time}

Des projets avec **minuteurs** démarrer/arrêter (ou plages ajoutées à la main), **budgets de temps** optionnels avec avertissements de dépassement, dates de fin prévues, et un **taux horaire** pour valoriser le temps. L'onglet **Répartition** trace les heures suivies par jour/semaine/mois, filtrable par projet et catégorie. Si un minuteur est resté actif pendant que l'application était fermée, il demande s'il faut conserver ou annuler ce temps.

## Mindset {#mindset}

Un **bilan** quotidien pour la psychologie du trader. Répondez à quelques questions avant ou après la séance : échelles (concentration, discipline), choix (calme / anxieux / FOMO), texte libre. Les questions sont **entièrement personnalisables** ; un jeu de départ est inclus. La vue **Tendances** trace vos réponses sur les derniers bilans, et l'Historique permet de relire n'importe quel jour.

## Prompt Store {#prompt-store}

Une bibliothèque pour les **prompts IA** que vous réutilisez : récaps de marché, questions de journal, modèles de recherche. Les prompts s'affichent en grille de vignettes (nom, tags, dernier enregistrement) avec une recherche sur le nom, les tags et le corps.

- **Tags** : ajoutez des tags libres dans l'éditeur ; filtrez la grille avec la barre de tags.
- **Noter et filtrer** : donnez un pouce levé ou baissé à un prompt et filtrez rapidement sur l'un ou l'autre.
- **Historique des versions** : chaque enregistrement est conservé ; ouvrez l'**Historique** d'un prompt pour prévisualiser n'importe quelle révision antérieure et y **revenir** (la restauration est enregistrée comme une nouvelle version, donc rien n'est perdu).
- **Dupliquer** : bifurquez un prompt pour créer une variante.
