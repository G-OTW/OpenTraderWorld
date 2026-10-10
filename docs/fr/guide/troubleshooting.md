# Dépannage

| Symptôme | Cause probable / solution |
|---|---|
| `port is already allocated` | Le port (80/443/5454) est utilisé par autre chose. Relancez `./setup.sh` et choisissez un autre port. |
| Chrome/Edge n'ouvre pas l'application mais Safari oui | Le navigateur force l'adresse en `https://`, que les modes HTTP simple ne servent pas. Tapez `http://` explicitement, ou passez en [mode Réseau local (LAN) + HTTPS](/fr/config/network#lan-https). |
| Fonctionne sur `localhost` mais pas via l'IP de la machine (macOS) | Le pare-feu macOS bloque les connexions entrantes de Docker. Réglages Système → Réseau → Pare-feu → Options… → réglez **Docker** sur *Autoriser les connexions entrantes*. |
| Réseau local (LAN) + HTTPS : certificat non émis | Vérifiez `docker compose logs caddy`. Le token DuckDNS/Cloudflare doit être valide et le domaine orthographié exactement. |
| Réseau local (LAN) + HTTPS : le domaine ne se résout pas sur certains appareils | Votre résolveur bloque les réponses en IP privée (protection contre le DNS rebinding). Voir [les solutions](/fr/config/network#dns-rebind). |
| L'assistant de configuration n'apparaît jamais / `core: offline` dans la barre du haut | Le core ne peut pas joindre Postgres. Vérifiez `docker compose logs core` et `logs postgres` ; vérifiez que `DATABASE_URL` correspond à `POSTGRES_PASSWORD` dans `deploy/.env`. |
| Erreur `POSTGRES_PASSWORD` au démarrage | `deploy/.env` est absent ou vide. Lancez `./setup.sh`, ou copiez `.env.example` vers `.env` et remplissez-le. |
| Mode public : certificat HTTPS non émis | Le DNS du domaine doit pointer vers ce serveur, et les ports 80/443 doivent être joignables depuis Internet. |
| Modifications de code non prises en compte | Reconstruisez : `docker compose up --build -d`. |
| Verrouillé dehors, mot de passe oublié | Réinitialisez-le depuis le shell de l'hôte, voir [Mot de passe oublié](#forgot-password). |
| Impossible de joindre l'application après avoir choisi le mauvais mode réseau | Modifiez `deploy/network.env` à la main et redémarrez, voir [changer le mode en ligne de commande](/fr/config/network#change-mode-cli). |

## Mot de passe oublié {#forgot-password}

Il n'y a ni e-mail de réinitialisation ni formulaire de réinitialisation sans authentification : OTW tourne sur votre propre serveur, donc tout point d'entrée capable de changer un mot de passe sans être connecté serait une porte d'entrée. La réinitialisation se fait donc depuis le **shell de l'hôte**, et le lien *Mot de passe oublié ?* de la page de connexion détaille les mêmes étapes.

Ouvrez un shell sur la machine qui fait tourner OTW et affichez un mot de passe à usage unique :

```bash
docker exec -it opentraderworld-core-1 /app/otw-core reset-password USERNAME
```

Connectez-vous avec ; l'application vous en demande immédiatement un nouveau.

| Cas | Commande à exécuter |
|---|---|
| Nom d'utilisateur oublié aussi | `docker exec opentraderworld-core-1 /app/otw-core list-users` |
| Choisir soi-même le mot de passe | `printf '%s' 'my-new-password' \| docker exec -i opentraderworld-core-1 /app/otw-core reset-password USERNAME --stdin` |
| Conteneur nommé autrement | `docker ps`, puis utilisez celui du core à la place de `opentraderworld-core-1`. |
| Pas de Docker | Lancez le binaire `otw-core` avec les mêmes arguments et `DATABASE_URL` défini. |

Ne passez jamais un mot de passe en argument de ligne de commande : la ligne de commande d'un processus est lisible sur l'hôte, c'est pourquoi `--stdin` existe.

Notes : une réinitialisation **déconnecte tous les appareils**, et rien n'est perdu. Le coffre et les clés de fournisseurs stockées sont scellés avec `OTW_SECRET_KEY`, pas avec votre mot de passe.

## Authentificateur perdu {#lost-authenticator}

Même principe que pour le mot de passe : récupérez l'accès depuis le shell de l'hôte.

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

Le second facteur est retiré et toutes les sessions sont déconnectées. Connectez-vous avec votre mot de passe,
puis reconfigurez-le sur le nouvel appareil depuis **Paramètres → Sécurité**. Voir
[Authentification à deux facteurs](/fr/config/security#totp).

## Verrouillé hors de la social login {#social-locked}

Le compte du fournisseur lié est verrouillé, supprimé ou injoignable : sur la page de connexion, **Utiliser un
code de récupération**. La connexion par mot de passe est réactivée.

Codes de récupération perdus aussi, sur l'hôte :

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Cela supprime le lien, réactive la connexion par mot de passe et déconnecte toutes les sessions. Mot de passe perdu
également : [Mot de passe oublié](#forgot-password). Détails : [Social login](/fr/config/social-login#rollback).

## Une longue requête est coupée {#request-timeout}

Un backtest, un balayage ou un gros import qui répond *this request took longer than the
Ns limit* a atteint le délai d'expiration des requêtes, ce n'est pas un bug. Augmentez-le dans **Paramètres → Sécurité** ; il s'applique
immédiatement, sans redémarrage. Voir [Délai d'expiration des requêtes](/fr/config/security#timeout).

## Lire les logs

```bash
cd deploy
docker compose ps              # are all containers up?
docker compose logs -f core    # API server
docker compose logs -f caddy   # proxy / certificates
docker compose logs -f postgres
```

L'application garde aussi sa propre vue des logs dans **Paramètres → Journaux** (avec recherche et niveau de capture configurable).

## Repartir de zéro

::: danger Cela supprime toutes les données
```bash
cd deploy
docker compose down -v
./setup.sh
```
:::

## Toujours bloqué ?

Ouvrez un ticket sur [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues) avec le symptôme et les lignes de log pertinentes.
