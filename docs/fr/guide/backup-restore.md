# Sauvegarde et restauration

Tout vit dans une seule base PostgreSQL, donc une sauvegarde est un simple `pg_dump`. **Paramètres → Sauvegarde et restauration** dans l'application affiche les mêmes commandes. Exécutez-les sur l'hôte où la pile est déployée, depuis le dossier d'installation (celui qui contient `deploy/`) ; elles utilisent le conteneur Postgres existant, aucun accès supplémentaire n'est nécessaire.

La section comporte deux onglets, chacun divisé en **Sauvegarde** et **Restauration** :

- **Complète** : toute la base, prise sur l'hôte, pour le jour où la machine meurt.
- **Partielle** : les modules que vous cochez, dans un seul zip, pour déménager ou garder une copie lisible.

## Sauvegarde complète

Dump simple :

```bash
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner > otw-backup-$(date +%F).sql
```

### La chiffrer (recommandé)

Un dump contient vos données en clair. Faites-le passer par `gpg` (ou `age`) pour que le fichier soit chiffré sur le disque. Une phrase secrète vous sera demandée ; `GPG_TTY` et `--pinentry-mode loopback` permettent à cette demande d'atteindre votre terminal pendant que le dump alimente l'entrée de gpg :

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner \
  | gpg --symmetric --cipher-algo AES256 --pinentry-mode loopback \
  > otw-backup-$(date +%F).sql.gpg
```

### Sauvegarde automatique {#automatic-backup}

Quand un script de sauvegarde sur l'hôte rend compte à l'application (l'[installateur serveur](#server-installs) en met un en place), l'onglet **Complète** s'ouvre sur une carte **Sauvegarde automatique** : depuis quand la dernière sauvegarde a tourné, où la copie est partie, si elle a été vérifiée, la planification et la taille. Elle passe en avertissement quand la dernière exécution a échoué, ou après 48 heures sans sauvegarde. Sans un tel script il n'y a pas de carte, et les commandes ci-dessus sont tout ce qu'il faut.

## Notes de sécurité

- Les **clés API et identifiants des fournisseurs** (flux d'actualités, fournisseurs de données de marché) sont déjà chiffrés au repos avec `OTW_SECRET_KEY`, ils n'apparaissent donc que sous forme chiffrée dans le dump.
- Sauvegardez **`OTW_SECRET_KEY`** (depuis `deploy/.env`) **séparément**, pas dans le même dump, sinon ces secrets chiffrés ne pourront pas être restaurés.
- Le dump inclut des **tokens de session** actifs. Traitez le fichier comme un secret, ou videz la table `sessions` après la restauration (`DELETE FROM sessions;`) et reconnectez-vous. Ne supprimez pas la table : la connexion en a besoin.
- Stockez la sauvegarde chiffrée **hors de la machine** et faites tourner les anciennes copies.

## Partielle (par module)

L'onglet **Partielle** prend les modules que vous cochez et vous donne **un seul fichier zip**, pour déplacer un journal vers une autre instance ou garder une copie lisible. Elle s'exécute application en marche, contrairement à la sauvegarde complète.

### Sauvegarde partielle

1. Ouvrez **Paramètres → Sauvegarde et restauration → Partielle → Sauvegarde**.
2. Cochez les modules voulus. Chacun affiche son nombre de lignes et sa taille, le total de la sélection s'affiche sous la liste, et *Contenu de la sélection* le détaille table par table. Les barres historiques sont décochées au départ : c'est de loin la plus grosse table, et elles peuvent être retéléchargées auprès de votre fournisseur.
3. Laissez **Inclure les identifiants de fournisseurs stockés** désactivé sauf si vous savez pourquoi vous en avez besoin. Ces valeurs sont chiffrées avec le `OTW_SECRET_KEY` de cette instance et illisibles ailleurs.
4. Cliquez sur **Télécharger les données sélectionnées**. Vous obtenez `otw-data-YYYY-MM-DD.zip`.

Dans le zip : `manifest.json` (ce qu'il contient, quelle version l'a écrit) et un `tables/<name>.jsonl` par table, un objet JSON par ligne. N'importe quel outil peut le lire.

### Restauration partielle

1. Ouvrez **Paramètres → Sauvegarde et restauration → Partielle → Restauration** sur l'instance cible et choisissez le fichier.
2. Le fichier est lu dès que vous le choisissez, et rien n'est écrit : vous obtenez la version qui l'a écrit et, par module et par table, le nombre de lignes qu'il contient face au nombre présent actuellement. Une très grande table est signalée par une estimation, marquée `~`. Un fichier que cette instance refuserait (endommagé, ou issu d'une version plus récente) est refusé à ce stade, avant tout engagement de votre part.
3. Choisissez comment il doit se combiner avec les données déjà présentes :
   - **Ajouter ce qui manque** conserve tout l'existant et n'ajoute que les lignes qui ne sont pas encore là. Rien n'est écrasé.
   - **Remplacer** efface les données de chaque module du fichier, puis charge la version du fichier. Il vous demande de saisir `REPLACE`, et enregistre d'abord une copie des données actuelles.

   La ligne sous le choix traduit ces comptes en ce qui va se passer. Remplacer est exact : *efface les N lignes d'ici, met à leur place les M lignes du fichier*. Fusionner ne peut donner qu'un plafond, *ajoute jusqu'à M lignes* : une ligne dont la clé est déjà là est ignorée, et seul le chargement lui-même sait combien cela représente.
4. Cliquez sur **Charger ce fichier**.

Tout se passe dans une seule transaction : si une partie échoue, rien n'est modifié.

::: warning Un fichier d'une version plus récente est refusé
Charger un paquet écrit par une version plus récente est refusé plutôt que tenté. Mettez d'abord l'instance à jour, puis chargez-le.
:::

Les lignes conservent leurs identifiants d'origine, et le fichier entier est lu en mémoire, donc une très grande sélection (typiquement les barres historiques) est refusée avec un message qui renvoie à la sauvegarde `pg_dump` ci-dessus. C'est le bon outil pour "tout, y compris ce que je ne regarde jamais".

## Restauration complète

Un dump ne se restaure que dans une base vide. Le core exécute ses migrations à chaque démarrage, donc une base sur laquelle il a déjà démarré (toute pile en marche, y compris celle que `setup.sh` vient de créer) entre en conflit avec le dump. Arrêtez le core, remplacez la base par une base vide, chargez le dump, redémarrez le core :

```bash
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q < otw-backup-YYYY-MM-DD.sql
docker compose -f deploy/docker-compose.yml start core
```

La ligne `DROP DATABASE` efface ce que la pile contient actuellement : faites-en d'abord un dump si cela compte. `ON_ERROR_STOP=1` s'arrête à la première erreur au lieu de charger un demi-dump.

Depuis une sauvegarde chiffrée :

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
gpg -d otw-backup-YYYY-MM-DD.sql.gpg \
  | docker compose -f deploy/docker-compose.yml exec -T postgres \
    psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q
docker compose -f deploy/docker-compose.yml start core
```

Assurez-vous que la pile restaurée utilise le **même `OTW_SECRET_KEY`** qu'au moment de la sauvegarde, sinon les identifiants de fournisseurs stockés seront illisibles.

## Installations serveur {#server-installs}

Une machine configurée avec l'[installateur serveur](/fr/guide/hosting/when-it-breaks#installer-options) dispose de la commande `otw`, qui fait tout ce qui précède pour vous :

| Commande | Ce qu'elle fait |
|---|---|
| `otw backup` | Fait une sauvegarde maintenant dans `/var/backups/otw` et vérifie que le fichier est complet. |
| `otw restore` | Liste les sauvegardes présentes sur la machine, demande laquelle, et la remet en place une fois que vous tapez `RESTORE`. Elle fait d'abord une sauvegarde complète des données actuelles, puis redémarre le core. |

L'installateur planifie aussi `otw backup` chaque nuit vers 03:20, ce qui fait apparaître la carte [Sauvegarde automatique](#automatic-backup). Une copie nocturne laisse de côté les deux tables qui peuvent être téléchargées à nouveau (le catalogue d'instruments et les barres historiques) ; la copie du dimanche est complète. Les 10 dernières copies nocturnes et les 2 dernières complètes sont conservées. Elles restent sur la machine : copiez-les ailleurs pour survivre à un serveur mort.
