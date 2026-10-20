# Mise à jour

OpenTraderWorld vous signale quand une nouvelle version est disponible dans **Paramètres → Mettre à jour l'app** (il interroge GitHub). L'application **ne peut pas se mettre à jour elle-même, par conception** (elle tourne sans accès au shell ni à Docker, pour limiter la surface d'attaque) : les mises à jour sont donc quelques commandes sur l'hôte.

## Avant de mettre à jour

1. **Faites une sauvegarde de la base** : voir [Sauvegarde et restauration](/fr/guide/backup-restore).
2. Parcourez les notes de version pour repérer d'éventuels changements incompatibles.

## Quelle installation avez-vous ?

Regardez votre dossier d'installation :

- Uniquement `deploy/` à l'intérieur, pas de `.git` : **installation par images** (l'installateur en une commande, ou
  `setup.sh` sans `--build`). C'est le cas par défaut.
- `core/`, `frontend/` et un `.git` : **build depuis les sources** (`install.sh --build`, ou un clone
  plus `./setup.sh --build`).

## Installation par images

Rien n'est construit et il n'y a pas de dépôt git, donc actualisez `deploy/` depuis la version publiée
(c'est là que les nouveaux tags d'images sont fixés), puis récupérez les images et redémarrez :

```bash
cd /path/to/opentraderworld
TMP=$(mktemp -d)
curl -fsSL https://codeload.github.com/G-OTW/OpenTraderWorld/tar.gz/master | tar -xz -C "$TMP"
cp -R "$TMP"/*/deploy/. deploy/
rm -rf "$TMP"
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  pull
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d
```

Votre configuration est conservée : `.env`, `network.env` et `dns.env` ne font pas partie de la
version publiée, la copie ne les écrase donc jamais. Tout le reste de `deploy/` est remplacé par
la nouvelle version, c'est le but : les modifications locales de `docker-compose.yml` ou de `Caddyfile`
sont perdues, gardez-les dans un patch si besoin.

## Build depuis les sources

Actualisez le dépôt, puis reconstruisez :

```bash
cd /path/to/OpenTraderWorld
git fetch origin
git reset --hard origin/master
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d --build
```

::: warning Utilisez `git reset --hard`, pas `git pull`
Chaque version est publiée comme un instantané neuf du dépôt, donc `git pull` signale
"divergent branches" et échoue. `git reset --hard origin/master` aligne exactement votre dépôt
sur la nouvelle version. Vos données et votre configuration ne sont pas touchées : elles vivent dans
les volumes Docker et dans `.env` / `network.env`, qui ne sont pas suivis par git. Si vous avez modifié
des fichiers suivis en local, mettez-les d'abord de côté (`git stash`).
:::

Pour récupérer les images publiées au lieu de reconstruire depuis votre dépôt, ajoutez
`-f deploy/docker-compose.images.yml` et utilisez `pull` + `up -d` comme pour l'installation par images.

C'est tout :

- Les conteneurs sont recréés avec la nouvelle version et redémarrés.
- Les **migrations de base de données s'exécutent automatiquement** au premier démarrage du nouveau conteneur core.
- Vos données ne sont pas touchées : elles vivent dans des volumes Docker, indépendants des images.

L'application est brièvement hors ligne pendant la recréation des conteneurs. Les commandes exactes (avec vos chemins configurés) sont aussi affichées dans **Paramètres → Mettre à jour l'app**.

## Ponctuel : OAuth pour les agents IA (0.0.16) {#oauth-caddyfile}

La connexion OAuth pour les clients MCP exige que les documents de découverte sous `/.well-known/oauth-*`
atteignent le core. La procédure d'installation par images ci-dessus et les builds depuis les sources apportent le nouveau
`deploy/Caddyfile` avec eux, tout comme les nouvelles installations. Une installation mise à jour avec `otw update` garde
son ancien Caddyfile : tout fonctionne sauf OAuth tant que vous n'ajoutez pas ce bloc, juste avant la ligne
`# Everything else → static frontend` :

```
	handle /.well-known/oauth-* {
		header Content-Security-Policy "default-src 'none'; frame-ancestors 'none'"
		reverse_proxy core:8080
	}
```

Puis recréez Caddy (un rechargement ne suffit pas : la plupart des éditeurs remplacent le fichier, et le
conteneur garde l'ancien) :

```bash
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d --force-recreate caddy
```

## Ponctuel : retirer le mot de passe d'amorçage {#retire-bootstrap-password}

Si le core affiche ceci au démarrage, c'est à vous qu'il s'adresse :

```
OTW_ADMIN_PASSWORD is still set but the admin account already exists.
```

`setup.sh` crée l'admin depuis `deploy/.env` au premier démarrage puis vide cette ligne. Sur
une installation faite avant qu'il ne le fasse, la valeur est toujours dans le fichier, et dans l'environnement
du conteneur où `docker inspect` la remet à quiconque peut joindre le démon Docker. Elle ne donne plus rien à ce stade,
c'est simplement une copie de plus d'un mot de passe sur le disque.

```bash
sed -i.bak 's/^OTW_ADMIN_PASSWORD=.*/OTW_ADMIN_PASSWORD=/' deploy/.env
chmod 600 deploy/.env && rm -f deploy/.env.bak
```

Puis recréez le core pour qu'il disparaisse aussi de l'environnement :

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d core
```

Gardez `OTW_ADMIN_USER` : un mot de passe vide rend tout l'amorçage sans effet, ce qui est le but, et la ligne
documente toujours comment une installation sans écran crée son premier compte.
