# Installation

Auto-hébergez OpenTraderWorld sur votre machine ou votre serveur. Compter environ 5 minutes.

::: info Conteneurs uniquement (pour l'instant)
OpenTraderWorld tourne comme une pile Docker Compose, le seul déploiement pris en charge. Une installation native est possible mais déconseillée : Docker garde l'installation non intrusive (tout vit dans des conteneurs et des volumes) et rapide à reconstruire. Voir [Installer Docker](/fr/guide/docker) pour les raisons et les étapes par système.
:::

## Prérequis

- **Docker** avec Docker Compose. Vous ne l'avez pas ? [Installer Docker](/fr/guide/docker) couvre macOS, Windows et Linux en quelques commandes.
- Linux, macOS ou Windows.
- Un port libre (**5454** par défaut ; les ports **80** + **443** pour les modes HTTPS). Vous pouvez le changer pendant la configuration. Les modes localhost et réseau local en HTTP simple occupent aussi le port **8443** sur `127.0.0.1`.

Vérifiez que Docker est prêt :

```bash
docker --version
docker compose version
```

## Installation en une commande (recommandée)

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

L'installateur vérifie que Docker est prêt, télécharge les fichiers de déploiement (uniquement le dossier `deploy/`, sans code source ni chaîne d'outils) dans `./opentraderworld`, puis passe la main à la configuration guidée ci-dessous, qui **récupère les images préconstruites** depuis Docker Hub.

Les options se placent après `bash -s --` :

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash -s -- --dir ~/otw
```

| Option | Par défaut | Notes |
|---|---|---|
| `--dir <path>` | `./opentraderworld` | Dossier d'installation. Refuse un dossier non vide ou une installation existante. |
| `--ref <ref>` | `master` | Branche ou tag à installer. |
| `--build` | désactivé | Clone tout le code source et construit les images localement au lieu de les récupérer (nécessite `git` et la chaîne d'outils). |
| `--yes` | désactivé | Transmis à `setup.sh` : prend toutes les valeurs par défaut quand aucun terminal ne permet de poser les questions. |
| `--wipe-volumes` | désactivé | Transmis à `setup.sh` : supprime les volumes d'une installation précédente sans demander. **Les données de cette installation sont effacées.** |

`OTW_DIR` et `OTW_REF` définissent les mêmes valeurs que `--dir` et `--ref` (une option l'emporte sur sa variable), et `OTW_REPO` le dépôt GitHub (par défaut `G-OTW/OpenTraderWorld`).

## Depuis un clone git (alternative)

```bash
git clone https://github.com/G-OTW/OpenTraderWorld.git
cd OpenTraderWorld/deploy
./setup.sh
```

## La configuration guidée

Les deux voies ci-dessus exécutent `deploy/setup.sh`. Il pose quelques questions, génère des secrets robustes, écrit la configuration et démarre tout.

Il demande :

| Question | Par défaut | Notes |
|---|---|---|
| **Mode réseau** | `1` (localhost) | `1` cette machine uniquement · `2` réseau local en HTTP simple · `3` réseau local en HTTPS avec un vrai certificat · `4` Internet public sur votre propre domaine. Modifiable ensuite, voir [Réseau et accès à distance](/fr/config/network). |
| **Nom d'utilisateur admin** | `admin` | Le compte admin est créé pour vous ; un mot de passe robuste est généré et affiché **une seule fois**. |
| **Port HTTP** | `5454` | Modes 1 et 2 uniquement ; les modes 3 et 4 utilisent 80 + 443. |
| **Domaine DuckDNS / token / IP du réseau local** | aucun | Mode 3 uniquement. |
| **Domaine public** | aucun | Mode 4 uniquement, et il doit déjà pointer vers ce serveur. |
| **Niveau de log** | `info` | `trace` / `debug` / `info` / `warn` / `error`. |

Il accepte aussi ces options :

| Option | Notes |
|---|---|
| `--build` | Construit les images depuis les sources au lieu de les récupérer (voir plus bas). |
| `--yes` | Prend toutes les valeurs par défaut quand aucun terminal ne permet de poser les questions. |
| `--wipe-volumes` | Supprime les volumes d'une installation précédente sans demander. **Les données de cette installation sont effacées.** |
| `--non-interactive` | Équivaut à `OTW_NONINTERACTIVE=1` : lit chaque réponse dans l'environnement, jamais depuis un terminal. Une variable non définie prend la valeur par défaut. |

Les variables du mode non interactif, une par question : `NET_CHOICE` (`1` à `4`), `OTW_HTTP_PORT`, `ADMIN_USER`, `OTW_ADMIN_PASSWORD` (généré si absent), `OTW_LOG`, `DUCK_DOMAIN`, `DUCK_TOKEN` et `LAN_IP` (mode 3), `OTW_DOMAIN` (mode 4), et `OVERWRITE`, `CLEAN_VOLS`, `WIPE_VOL`, `START_NOW` (`y` ou `N`) pour les confirmations.

Les **secrets de base de données et de session sont générés automatiquement**, vous ne les saisissez donc jamais. Ils sont écrits dans `deploy/.env` (permissions de fichier `600`, jamais commité dans git).

À la fin, laissez le script démarrer la pile : il attend l'API, **crée votre compte admin** et affiche le mot de passe généré **une seule fois**, copiez-le donc avant de fermer le terminal.

Par défaut, la configuration **récupère les images préconstruites** depuis Docker Hub : pas de chaîne d'outils Rust ou Node, premier démarrage en quelques minutes. Lancez `./setup.sh --build` pour construire les trois services depuis les sources (développement, modifications locales).

::: tip Serveurs sans écran
L'admin est créé par le core lui-même au premier démarrage (depuis `deploy/.env`), vous n'avez donc pas besoin de navigateur sur le serveur. Notez le mot de passe affiché et connectez-vous depuis n'importe quelle machine qui peut joindre l'application.
:::

::: warning Réinstaller par-dessus d'anciennes données
Si des volumes Docker d'une installation précédente existent, la configuration propose de les effacer. La réponse par défaut est **Non** partout : l'effacement (et la perte de l'ancienne base) n'a lieu que sur un `y` explicite. De nouveaux secrets sur un ancien volume de base ne peuvent pas fonctionner, donc refuser interrompt la configuration plutôt que de démarrer une pile cassée.
:::

## Installation manuelle (alternative)

Si vous préférez configurer à la main :

```bash
cd deploy
cp .env.example .env
```

Modifiez `.env` et définissez au moins :

- `POSTGRES_PASSWORD` : un mot de passe robuste
- `DATABASE_URL` : doit contenir le même mot de passe, p. ex. `postgres://otw:YOUR_PASSWORD@postgres:5432/opentraderworld`
- `SESSION_SECRET` : une longue chaîne aléatoire
- `OTW_SECRET_KEY` : une autre longue chaîne aléatoire, la clé qui chiffre les identifiants stockés. Gardez-la stable : la changer plus tard les rend illisibles.

Compose a aussi besoin des deux fichiers réseau que `setup.sh` aurait écrits. Pour le mode localhost sur le port 5454 :

```bash
cat > network.env <<'EOF'
OTW_BIND=127.0.0.1
OTW_HTTPS_BIND=127.0.0.1
OTW_HTTP_PORT=5454
OTW_HTTPS_PORT=8443
OTW_DOMAIN=:80
EOF
printf 'OTW_ACME_GLOBAL=\n' > dns.env
```

Les deux sont réécrits ensuite depuis **Paramètres → Réseau**, voir [Réseau et accès à distance](/fr/config/network).

Puis démarrez la pile. Par défaut, cela **récupère les images préconstruites** depuis Docker Hub (aucune chaîne d'outils Rust ou Node requise) :

```bash
docker compose -f docker-compose.yml -f docker-compose.images.yml \
  --env-file .env --env-file network.env up -d
```

::: details Construire depuis les sources à la place
Pour le développement, ou pour exécuter des modifications locales, omettez la surcharge d'images et construisez vous-même les trois services (nécessite la chaîne d'outils ; la compilation Rust est lente) :

```bash
docker compose --env-file .env --env-file network.env up --build -d
```
:::

## Créer l'admin (installations manuelles uniquement)

Si vous avez utilisé `./setup.sh` et l'avez laissé démarrer la pile, **votre admin existe déjà**, passez donc à la suite.

Sinon, ouvrez l'application dans votre navigateur (`http://localhost:5454` par défaut). À la première visite, OpenTraderWorld détecte qu'il n'y a pas encore d'admin et affiche l'**assistant de configuration** : choisissez un nom d'utilisateur et un mot de passe, validez, et vous arrivez sur le dashboard. Le mot de passe doit faire au moins 12 caractères, et un caractère répété, une suite comptée, un mot de passe contenant le nom d'utilisateur ou figurant dans une liste de fuites connues est refusé. Les mots de passe sont stockés hachés avec argon2.

Dans les modes HTTPS (réseau local + HTTPS, public), l'assistant demande aussi un **jeton de configuration**, pour que personne ne puisse réclamer le compte en premier simplement en trouvant l'adresse. Le core l'affiche dans son log au démarrage : `docker compose logs core`.

::: details Créer l'admin en ligne de commande (sans écran, sans navigateur)
Appelez le point d'entrée de premier lancement depuis l'intérieur de la pile : cela fonctionne quelle que soit votre interface de liaison, et refuse (HTTP 409) si un admin existe déjà. Dans les modes HTTPS, ajoutez le jeton de configuration lu dans le log du core comme en-tête supplémentaire, `--header=X-OTW-Setup-Token:<token>`, sinon la réponse est 403 `setup_token_required` :

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T caddy \
  wget -qO- --header=Content-Type:application/json \
  --post-data='{"username":"admin","password":"CHOOSE-A-STRONG-ONE"}' \
  http://core:8080/api/setup
```
:::

## Vérifier que ça tourne

- Application : `http://localhost:5454` (ou le port/domaine choisi)
- Contrôle de santé : `http://localhost:5454/api/health` → `{"status":"ok"}` (une fois connecté, il renvoie aussi la version, la durée de fonctionnement et l'état de chaque service)

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## Opérations courantes

Exécutez-les depuis `deploy/`. Chaque commande prend les deux fichiers d'environnement, et une installation par images prend aussi la surcharge d'images, partez donc de la ligne qui correspond à votre installation :

```bash
# image install (the default)
docker compose -f docker-compose.yml -f docker-compose.images.yml --env-file .env --env-file network.env
# source build
docker compose --env-file .env --env-file network.env
```

| Action | Ajouter |
|---|---|
| Démarrer | `up -d` |
| Arrêter | `down` |
| Voir les logs | `logs -f` |
| Reconstruire après un changement de code (build depuis les sources) | `up --build -d` |
| Arrêter **et effacer toutes les données** | `down -v` |

Pour passer à une version plus récente, suivez [Mise à jour](/fr/guide/updating) : les tags des images sont figés dans `docker-compose.images.yml`, donc un simple `pull` récupère la version que vous avez déjà.

Vos données vivent dans des volumes nommés Docker et **persistent** entre les `up`/`down`. Elles ne sont supprimées qu'avec `down -v`.

## Étapes suivantes

- [Premiers pas](/fr/guide/first-steps) : connexion, valeurs par défaut, installation des modules.
- [Réseau et accès à distance](/fr/config/network) : joindre l'application depuis d'autres appareils, HTTPS sur réseau local, exposition publique.
- Un problème ? Voir [Dépannage](/fr/guide/troubleshooting).
