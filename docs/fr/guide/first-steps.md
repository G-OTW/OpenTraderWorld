# Premiers pas

Vous avez [installé](/fr/guide/install) OpenTraderWorld et vous avez vos identifiants admin. Voici comment le personnaliser.

## Se connecter

Ouvrez l'application et connectez-vous sur `/login`. Si votre mot de passe a été **généré par l'installateur**, il vous est demandé d'en **choisir un nouveau à la première connexion**, car le mot de passe généré ne fonctionne qu'une fois.

Vous pouvez changer votre nom d'utilisateur ou votre mot de passe à tout moment dans **Paramètres → Compte**. Changer votre mot de passe vous déconnecte de toutes les sessions. Verrouillé dehors ? Il n'y a pas d'e-mail de réinitialisation : récupérez l'accès depuis le shell de l'hôte, voir [Mot de passe oublié](/fr/guide/troubleshooting#forgot-password).

Ensuite, dans **Paramètres → Sécurité** : activez l'**authentification à deux facteurs** et vérifiez quels navigateurs sont connectés. Faites-le avant de laisser quoi que ce soit au-delà de cette machine atteindre l'application. Voir [Sécurité du compte](/fr/config/security).

## Définir vos valeurs par défaut

Allez dans **Paramètres → Valeurs par défaut** et choisissez :

- **Langue** : s'applique immédiatement à toute l'application (anglais, français, allemand, espagnol, italien, portugais, chinois).
- **Devise par défaut** et **fuseau horaire** : utilisés comme valeurs de départ dans tous les modules.

## Installer vos modules

Ouvrez **Paramètres → Modules**. Tous les modules sont livrés avec l'application ; en installer un le rend simplement disponible dans le sélecteur de modules et sur le dashboard, et rien n'est téléchargé.

- **Installez** les modules que vous voulez. Commencez petit, vous pouvez en ajouter à tout moment.
- Certains modules dépendent d'autres : **Historical Data Visualization**, **Backtest** et **Quant Tools** ont tous besoin de **Historical Data** (ils travaillent sur ses jeux de données téléchargés).
- **Détacher** masque un module et le rend inaccessible ; ses données sont conservées sauf si vous cochez aussi *supprimer les données*. Vous pouvez le réinstaller à tout moment.

Vous ne savez pas par où commencer ? Voir la [vue d'ensemble des modules](/fr/modules/) pour ce que fait chacun.

## S'orienter dans l'application

- **Sélecteur de modules** (en haut à gauche) : passe d'un module installé à un autre. Chaque module possède toute sa zone de travail : sa propre barre latérale, ses pages et son contenu.
- **Dashboard** (accueil) : un tableau de tuiles et de widgets de vos modules, avec autant de pages que vous voulez. Voir [Dashboard et navigation](/fr/modules/dashboard).
- **Recherche** (barre du haut, <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, ou <kbd>/</kbd>) : trouve les modules et les sections des paramètres ; le bouton de couches l'élargit à votre propre contenu.
- **Cloche** (barre du haut) : la boîte de notifications, où arrivent les rappels et les alertes de webhooks.
- **Paramètres** : compte, valeurs par défaut, apparence, réseau, modules, données, sauvegarde, mises à jour, journaux, connectors, coffre et plus. Voir la [référence des paramètres](/fr/config/settings).

## Étapes recommandées

1. **Prenez tôt l'habitude de sauvegarder** : voir [Sauvegarde et restauration](/fr/guide/backup-restore).
2. Si d'autres appareils doivent joindre l'application, lisez [Réseau et accès à distance](/fr/config/network) avant de rien changer, et activez d'abord l'[authentification à deux facteurs](/fr/config/security#totp).
3. Vous utilisez des fournisseurs de données de marché externes ? Créez un **[connector de données](/fr/config/connectors)** par compte dans **Paramètres → Data connectors** selon vos besoins. L'application fonctionne très bien sans, et plusieurs fournisseurs n'exigent aucune clé.
