# Réseau et accès à distance

OpenTraderWorld contrôle **qui peut joindre l'application** grâce à quatre modes réseau. Après l'installation, il est **en localhost uniquement** : rien sur votre réseau ne peut s'y connecter tant que vous ne changez pas cela.

La méthode prise en charge pour changer de mode est dans l'application : **Paramètres → Réseau**. L'enregistrement affiche la commande de redémarrage exacte à exécuter sur l'hôte (l'application ne peut pas redémarrer ses propres conteneurs) ; la pile est brièvement hors ligne pendant la recréation des conteneurs.

## Les quatre modes

| Mode | Joignable par | Protocole | À utiliser quand |
|---|---|---|---|
| **Local uniquement** | cette machine | HTTP | Par défaut. Le plus sûr, rien sur le réseau ne peut s'y connecter. |
| **Réseau local (LAN)** | les appareils de votre réseau, via l'IP de la machine | HTTP simple | Accès rapide sur le réseau local ; acceptable sur un réseau domestique de confiance. Les navigateurs peuvent avertir ou forcer le HTTPS, et ils refusent le microphone, donc la [commande vocale](/fr/config/voice#https) ne fonctionne pas depuis les autres appareils. |
| **LAN + HTTPS** | les appareils de votre réseau, via un vrai domaine | HTTPS, certificat de confiance | Accès réseau local sans avertissements du navigateur. Rien n'est exposé à Internet. |
| **Public (Web)** | n'importe qui, sur votre domaine | HTTPS (Let's Encrypt) | Vous voulez un accès de partout et acceptez l'exposition publique. |

## LAN + HTTPS (sans avertissements du navigateur) {#lan-https}

Les navigateurs refusent ou avertissent de plus en plus sur les sites en HTTP simple. Ce mode sert l'application à tous les appareils de votre réseau avec un **vrai certificat, publiquement reconnu** : aucun avertissement, rien à installer sur les appareils clients, et **rien d'exposé à Internet**. La propriété du domaine est prouvée par un enregistrement DNS (défi ACME DNS-01), pas par une connexion entrante, et le domaine pointe vers l'IP privée de votre machine sur le réseau local.

Configurez-le à l'installation (`./setup.sh`, mode `3`) ou plus tard dans **Paramètres → Réseau → Réseau local (LAN) + HTTPS** :

1. Connectez-vous sur [duckdns.org](https://www.duckdns.org) (gratuit), ajoutez un sous-domaine (p. ex. `myotw.duckdns.org`) et copiez le token de votre compte, ou utilisez votre propre domaine sur Cloudflare avec un token d'API autorisant l'édition du DNS.
2. Saisissez le domaine + le token, ainsi que l'IP LAN de votre machine (avec DuckDNS l'enregistrement y est pointé automatiquement ; sur Cloudflare, créez vous-même l'enregistrement A).
3. Appliquez avec la commande de redémarrage affichée. La première requête peut prendre ~30 s le temps que le certificat soit émis.

Ouvrez ensuite `https://myotw.duckdns.org` depuis n'importe quel appareil de votre réseau.

::: warning Remarques
- Les certificats émis apparaissent dans les journaux publics de Certificate Transparency, donc le **nom** du domaine est publiquement visible (l'application elle-même reste limitée au réseau local).
- Le token DNS est stocké dans `deploy/dns.env` : ne commitez ni ne partagez jamais ce fichier.
- Ce mode utilise les ports **80 + 443** au lieu du port personnalisé.
:::

### Si le domaine ne se résout pas sur certains appareils {#dns-rebind}

Certains routeurs/résolveurs de fournisseurs d'accès suppriment silencieusement les réponses DNS pointant vers une IP privée ("DNS rebind protection"). Solutions, de la meilleure à la moins bonne :

1. **Autorisez le domaine** dans les réglages de votre routeur/DNS.
2. **Activez le DNS sécurisé (DNS over HTTPS)** dans le navigateur. Chrome : Paramètres → Confidentialité et sécurité → Sécurité → *Utiliser un DNS sécurisé* → Cloudflare ; Firefox : Paramètres → Vie privée → *DNS over HTTPS* → Protection maximale.
3. **Contournement par fichier hosts** (par machine, et les téléphones ne peuvent pas le faire). Associez le domaine à l'IP LAN du serveur :

   ```bash
   # macOS / Linux, then flush the cache (macOS only):
   echo "192.168.1.50 myotw.duckdns.org" | sudo tee -a /etc/hosts
   sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder
   ```

   Sous Windows, modifiez `C:\Windows\System32\drivers\etc\hosts` en tant qu'administrateur, ajoutez la même ligne, puis lancez `ipconfig /flushdns`. Le fichier hosts l'emporte toujours sur le DNS, supprimez donc la ligne si l'IP du serveur change.

## Public (Web) {#public}

Expose l'application sur votre propre domaine avec HTTPS automatique.

**Prérequis :**

1. Un **domaine** avec un **enregistrement A / AAAA** public pointant vers l'IP publique de votre serveur.
2. Le TCP **80** et **443** entrants qui atteignent le serveur : ouvrez-les dans le pare-feu du routeur/du cloud et redirigez les ports si vous êtes derrière un NAT. Le port 80 est requis pour le certificat (défi HTTP-01) et redirige vers HTTPS.

Choisissez le mode `4` dans `./setup.sh` ou basculez dans **Paramètres → Réseau**. Caddy obtient et renouvelle automatiquement un certificat Let's Encrypt à la première requête (~30 s).

::: danger N'importe qui peut atteindre la page de connexion une fois ce mode activé
Ne l'activez qu'une fois votre compte admin créé et avec un mot de passe robuste. Activez
l'[authentification à deux facteurs](/fr/config/security#totp) avant. Gardez `deploy/.env` secret.
Revenez à un mode privé à tout moment dans Paramètres → Réseau.
:::

## Le token de configuration {#setup-token}

Sur **LAN + HTTPS** et **Public**, l'assistant de premier lancement demande un **token de configuration** avant de
créer le premier compte.

La raison est une course : ces modes répondent au réseau, et l'assistant reste ouvert tant qu'aucun compte
n'existe. Sans token, la première personne qui charge la page devient administrateur, ce qui est un
vrai risque pendant la propagation du DNS, et de nouveau si un volume de base de données est un jour recréé.

Le core génère le token au démarrage et l'écrit dans son log :

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env logs core | grep "setup token"
```

Collez-le dans le champ supplémentaire que montre l'assistant. Un nouveau est généré à chaque redémarrage, donc un
token abandonné cesse de fonctionner dès que le conteneur redémarre.

Vous ne le verrez pas si vous avez installé avec `./setup.sh` : il crée le compte depuis
`deploy/.env` au premier démarrage, avant que quoi que ce soit puisse atteindre l'assistant. Le token n'apparaît
que si aucun compte n'existe encore.

## Changer de mode en ligne de commande {#change-mode-cli}

Si vous avez choisi le mauvais mode à l'installation et ne pouvez plus joindre l'application (p. ex. vous avez choisi *localhost* sur un serveur sans écran), modifiez directement `deploy/network.env` et redémarrez :

```bash
cd deploy
# make it reachable on your LAN over plain HTTP (mode 2):
#   OTW_BIND=0.0.0.0     (was 127.0.0.1)
#   OTW_HTTP_PORT=5454   (or your chosen port)
$EDITOR network.env
docker compose --env-file .env --env-file network.env up -d
```

`network.env` ne contient aucun secret : il contient l'interface de liaison (`127.0.0.1` = cette machine uniquement, `0.0.0.0` = toutes les interfaces) et les ports, interpolés par Compose. LAN + HTTPS demande plus d'une ligne (certificat + token DNS), configurez donc ce mode depuis les Paramètres ou `./setup.sh` mode `3`. Une fois l'application ouverte, utilisez **Paramètres → Réseau**.
