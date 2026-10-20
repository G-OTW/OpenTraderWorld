# Qu'est-ce que OpenTraderWorld ?

> Site du projet : **[opentraderworld.com](https://opentraderworld.com)** : visite des modules,
> [démo en ligne](https://demo.opentraderworld.com), [guides de la communauté](https://opentraderworld.com/docs)
> et [vote sur la feuille de route](https://opentraderworld.com/suggestions).

OpenTraderWorld est une **plateforme web auto-hébergée pour traders et investisseurs**. Vous l'installez une fois avec Docker sur votre ordinateur ou votre serveur, l'ouvrez dans un navigateur, et obtenez un espace privé composé de modules : un journal de trading, des données de marché historiques avec graphiques et backtesting, le suivi de portefeuille et de patrimoine, un agrégateur d'actualités, des notes, des listes de contrôle et plus encore.

**Gratuit pour tous, usage personnel ou professionnel. Code source disponible (FSL-1.1-MIT).** La seule chose interdite est de le revendre ou de le proposer comme service payant. Le principe directeur : *devenez rentable avant de dépenser un centime.*

## Pourquoi l'auto-hébergement ?

- **Vos données restent les vôtres.** Trades, portefeuilles, notes et entrées de journal vivent dans une base PostgreSQL sur votre machine, pas sur le serveur de quelqu'un d'autre.
- **Privé par défaut.** Après l'installation, l'application n'écoute que sur `localhost`. L'exposer à votre réseau local ou à Internet est un choix explicite que vous faites dans [Paramètres → Réseau](/fr/config/network).
- **Aucun abonnement.** Les outils de base ne coûtent rien à faire tourner. Certains modules peuvent utiliser en option des fournisseurs de données externes (beaucoup avec des offres gratuites), et vous apportez vos propres clés API.

## Fonctionnement

Une seule pile `docker compose`, quatre services :

| Service | Rôle |
|---|---|
| **core** | Serveur d'API Rust (Axum) : toute la logique métier, le planificateur, les tâches de fond |
| **postgres** | PostgreSQL : le seul endroit où vivent vos données |
| **frontend** | Application monopage SvelteKit, construite une fois au déploiement |
| **caddy** | Reverse proxy : sert l'application, relaie `/api`, gère les certificats HTTPS |

L'application est **mono-utilisateur** : un seul compte administrateur, créé à l'installation. Il n'y a ni mode multi-tenant, ni partage, ni gestion d'utilisateurs à configurer.

Docker est actuellement le **seul déploiement pris en charge** : il garde l'installation non intrusive et rapide à reconstruire ([pourquoi, et comment obtenir Docker](/fr/guide/docker)). Une installation native est possible mais déconseillée.

## Les modules

Les modules sont des ensembles de fonctionnalités que vous installez ou détachez dans **Paramètres → Modules**. Tout est livré avec l'application, et installer un module ne fait que l'activer. Points forts :

- **[Journal de trading](/fr/modules/journal)** : consignez vos trades avec des modèles, des barèmes de frais, un PnL multi-devises et des statistiques de performance complètes.
- **[Données de marché et backtesting](/fr/modules/market-data)** : téléchargez l'historique OHLCV auprès de plusieurs fournisseurs, affichez n'importe quel instrument en direct ou à la demande avec des indicateurs, backtestez des stratégies à base de règles, et lancez des analyses quantitatives sur des jeux de données, des backtests enregistrés, des courbes de futures et des chaînes d'options.
- **[Portefeuilles et patrimoine](/fr/modules/portfolio)** : suivi de portefeuille en direct, historique de valeur nette, positions 13F des superinvestisseurs, estimations fiscales.
- **[Actualités et recherche](/fr/modules/news-research)** : tableaux de bord d'actualités RSS/API, calendrier économique, un catalogue de recherche de 300 000 instruments.
- **[Notes et organisation](/fr/modules/productivity)** : éditeur de texte enrichi avec bases de données, tâches, objectifs, calendrier, rappels, routines de trading et bilans d'état d'esprit.
- **[Agent IA](/fr/modules/agent)** : assistant de chat intégré (apportez votre propre fournisseur) qui peut agir sur vos données via MCP, avec mémoire, compétences et serveurs MCP externes.

Voir la [liste complète des modules](/fr/modules/).

## Étapes suivantes

1. [Installer OpenTraderWorld](/fr/guide/install) : environ 5 minutes avec Docker.
2. [Faire vos premiers pas](/fr/guide/first-steps) : connexion, choix des valeurs par défaut, installation des modules.
3. [Configurer l'accès réseau](/fr/config/network) : si vous voulez y accéder depuis d'autres appareils.

Pas prêt à installer ? Essayez la [démo en ligne](https://demo.opentraderworld.com), une instance partagée
pré-remplie de données, réinitialisée toutes les 15 minutes. [Ce qu'est le mode démo](/fr/guide/demo), et ce qu'il bloque.
