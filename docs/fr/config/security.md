# Sécurité du compte

OTW a un seul compte et aucun e-mail de réinitialisation de mot de passe. Ce qui le protège, c'est le mot de passe (ou un
compte social lié), un second facteur optionnel, et le fait que rien d'autre que vous ne peut
joindre la machine. Cette page
couvre ce que vous pouvez activer et que faire quand quelque chose tourne mal.

Tout ceci se trouve dans **Paramètres → Sécurité**, sauf le mot de passe lui-même, qui reste dans
**Paramètres → Compte**.

## Authentification à deux facteurs {#totp}

Un code à six chiffres issu d'une application sur votre téléphone, demandé après le mot de passe. C'est ce qu'un
mot de passe volé seul ne peut pas franchir, et c'est la chose la plus utile à activer
avant d'ouvrir l'application à Internet.

::: tip Rien ne vous est envoyé
C'est du **TOTP** (RFC 6238), pas un code envoyé par SMS ou par e-mail à la demande. Votre application d'authentification et
le serveur partagent un secret une fois, à la configuration, puis calculent chacun le même code à partir de l'heure
courante. Pas de SMS, pas d'e-mail, pas de service tiers, et cela fonctionne avec la machine hors ligne.
:::

### L'activer

1. **Paramètres → Sécurité → Configurer**. L'application affiche un QR code, le lien `otpauth://` qui se cache
   derrière, et le secret lui-même.
2. Ajoutez-le à n'importe quelle application d'authentification : Google Authenticator, Aegis, Ente Auth, 1Password,
   Bitwarden, Proton Pass, celle que vous utilisez déjà. Scannez le QR code, collez le lien,
   ou saisissez le secret à la main.
3. Saisissez les six chiffres affichés et confirmez.

Rien ne change à la connexion tant que cette dernière étape n'a pas réussi. Un code que vous ne pouvez pas produire
ne devient jamais une exigence, donc une configuration inachevée ne peut pas vous verrouiller dehors.

### Se connecter ensuite

Saisissez votre nom d'utilisateur et votre mot de passe comme avant ; l'application demande alors le code. Chaque code ne fonctionne
qu'une fois et dure 30 secondes, avec une petite tolérance de chaque côté pour un téléphone dont l'horloge
a dérivé.

### Le désactiver

**Paramètres → Sécurité → Désactiver**, ce qui redemande votre mot de passe. Faites-le avant
d'effacer un téléphone, et reconfigurez-le sur le nouveau.

### Si vous perdez l'authentificateur {#totp-lost}

Il n'y a ni code de secours ni e-mail de récupération, pour la même raison qu'il n'y a pas de formulaire de réinitialisation
du mot de passe. Récupérez l'accès depuis le shell de la machine :

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

Le second facteur et son secret sont supprimés et toutes les sessions sont déconnectées. Connectez-vous avec
votre mot de passe, puis reconfigurez-le sur le nouvel appareil.

::: warning Gardez un moyen de revenir
Stockez le secret dans votre gestionnaire de mots de passe lors de la configuration, ou gardez activée la sauvegarde
propre à votre application d'authentification. Sinon, le seul moyen de revenir est l'accès shell à la machine.
:::

## Verrou de trading {#trading-lock}

Verrou actif, être connecté ne suffit pas pour envoyer des ordres. Le premier ordre après la
connexion demande un code de votre authentificateur, une seule fois, puis le trading reste
ouvert sur tous les comptes de courtage dans ce navigateur jusqu'à la déconnexion. Un autre
navigateur ou une nouvelle connexion le redemande.

- **L'activer** : **Paramètres → Sécurité → Verrou de trading → Activer**. L'authentification
  à deux facteurs doit être configurée avant, le verrou s'ouvre avec elle.
- **Ce qu'il couvre** : passer un ordre, modifier un ordre en cours et clôturer une position.
  Annuler un ordre, ou tous avec **Annuler tous les ordres**, ne le demande jamais : le verrou
  n'empêche jamais de réduire le risque.
- **Le désactiver** : demande votre mot de passe et un code, comme les autres
  [actions confirmées](#step-up), pour qu'une session volée ne puisse pas le couper d'abord.
- **Codes erronés** : cinq depuis un navigateur bloquent ce navigateur jusqu'à 15 minutes. Dès
  le troisième, une notification part comme une alerte de connexion.
- Les agents IA et les workflows de l'Automator ne peuvent pas envoyer d'ordres du tout,
  verrou ou non.

### Le réinitialiser {#trading-lock-reset}

Authentificateur perdu ? Retirez le second facteur avec `disable-totp`
[comme ci-dessus](#totp-lost). Verrou actif, cela désactive aussi le verrou et coupe le
trading sur **tous** les comptes de courtage, pour que rien ne reste ouvert sans facteur.
Désactiver l'authentification à deux facteurs depuis les Paramètres fait de même. Pour trader
à nouveau : connectez-vous, configurez l'authentification à deux facteurs, réactivez le
trading compte par compte dans **Paramètres → Brokers** (chacun demande votre mot de passe),
puis réactivez le verrou.

## Social login {#social}

Connexion Google, Microsoft, GitHub ou OpenID Connect, verrouillée sur un seul compte lié, avec
codes de récupération et un interrupteur optionnel pour désactiver les mots de passe. Voir
[Social login](/fr/config/social-login).

## Sessions actives {#sessions}

**Paramètres → Sécurité** liste chaque navigateur actuellement connecté au compte, avec l'
adresse d'où il vient et sa dernière utilisation. Celui dans lequel vous lisez ceci est marqué.

- **Fermer** met fin à l'une d'elles.
- **Fermer toutes les autres sessions** met fin à toutes sauf la vôtre, ce qu'il faut cliquer si vous vous êtes connecté
  quelque part où vous n'auriez pas dû, ou si vous avez un doute.

Une connexion depuis une adresse que le compte n'a jamais utilisée déclenche aussi une notification. Elle
arrive dans la cloche, et est poussée vers tout canal accordé au producteur **Sécurité (connexions, jetons,
exposition)** dans **Paramètres → Notifications** (une autorisation générique la couvre déjà). Elle se déclenche une fois
par adresse, pas à chaque connexion.

Avec **Se souvenir de moi** coché à la connexion, une session dure une semaine. Sans, elle dure une journée
et prend fin à la fermeture du navigateur. Sur les modes exposés au réseau (**LAN + HTTPS** et **Public**), une session
prend aussi fin après une journée sans activité, pour qu'un navigateur laissé ouvert sur une machine que vous avez quittée
ne reste pas connecté indéfiniment. En localhost et en réseau local simple, seule la limite fixe
s'applique.

## L'invite de mot de passe qui revient {#step-up}

Quelques actions redemandent votre mot de passe alors que vous êtes déjà connecté :

- créer un token d'accès pour un agent IA (**Paramètres → MCP**)
- activer la connexion OAuth des agents IA, et approuver chaque page de consentement OAuth
- changer le mode réseau (**Paramètres → Réseau**)
- écrire une valeur dans le **Coffre**
- télécharger une sauvegarde partielle **avec les identifiants inclus**
- désactiver le second facteur, ou le configurer à nouveau alors qu'il est actif
- désactiver le verrou de trading
- modifier le [social login](/fr/config/social-login) : ses réglages de fournisseur, lier ou
  délier le compte, de nouveaux codes de récupération, activer ou désactiver la connexion par mot de passe

Elles créent soit un identifiant, soit changent ce que le monde extérieur peut atteindre, soit vous remettent tous les
secrets stockés dans un seul fichier. Une confirmation les couvre toutes pendant cinq minutes, donc une série de
changements ne demande qu'une fois, et la confirmation appartient au navigateur qui l'a donnée. Si le second
facteur est activé, l'invite demande aussi un code.

Activer le trading sur un compte broker, modifier ses limites de risque ou la **Règle d'arrêt**, et
lever un arrêt demandent le mot de passe à chaque fois : la fenêtre de cinq minutes ne compte pas pour
elles.

## Règles de mot de passe {#password}

Défini ou modifié dans **Paramètres → Compte**, qui demande toujours d'abord le mot de passe actuel
et, en cas de succès, déconnecte toutes les sessions, y compris celle en cours.

Un mot de passe doit comporter **au moins 12 caractères** et ne doit pas figurer dans les
listes publiques de fuites. Cette vérification ignore la décoration que l'on ajoute pour contourner une règle, donc
`P@ssw0rd!2024` est refusé pour la même raison que `password`. Les suites (`abcdefghijkl`) et
tout ce qui contient le nom de votre compte sont aussi refusés.

Le mot de passe le plus simple qui passe est quelques mots sans rapport : `fennel-ladder-oxide-73` est
accepté, court et facile à taper. Il n'y a pas de règle sur le mélange de symboles et de chiffres, car
c'est la longueur qui compte.

Oublié ? Voir [Mot de passe oublié](/fr/guide/troubleshooting#forgot-password).

## Délai d'expiration des requêtes {#timeout}

**Paramètres → Sécurité** règle aussi combien de temps une seule requête peut s'exécuter avant que le serveur ne l'arrête.
Le défaut est de 120 secondes.

Il existe pour qu'une requête bloquée ne garde pas une connexion ouverte indéfiniment. Ce n'est **pas** une limite de
débit : rien ne compte la fréquence de vos appels à l'application, et un agent IA qui sollicite fortement l'API n'est
pas affecté. Les flux de prix en direct et la vue des logs ne sont pas affectés non plus, car la limite couvre
la production d'une réponse, pas la durée de vie d'un flux.

Augmentez-le si un long backtest ou un gros import est coupé avec un message de délai dépassé.

## Premier lancement sur une instance exposée au réseau {#setup-token}

Quand le tout premier compte est créé sur une instance déjà joignable depuis le réseau
(**LAN + HTTPS** ou **Public**), l'assistant de configuration demande un **token de configuration**. Voir
[Réseau et accès à distance](/fr/config/network#setup-token).
