# Sauvegarde et restauration

Tout vit dans une seule base PostgreSQL, donc une sauvegarde est un simple `pg_dump`. **Paramètres → Sauvegarde et restauration** dans l'application affiche ces commandes pré-remplies pour votre déploiement. Exécutez-les sur l'hôte où la pile est déployée ; elles utilisent le conteneur Postgres existant, aucun accès supplémentaire n'est nécessaire.

La section comporte deux onglets, chacun divisé en **Sauvegarde** et **Restauration** :

- **Complète** : toute la base, prise sur l'hôte, pour le jour où la machine meurt.
- **Partielle** : les modules que vous cochez, dans un seul zip, pour déménager ou garder une copie lisible.

## Sauvegarde complète

Dump simple :

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld > otw-backup-$(date +%F).sql
```

### La chiffrer (recommandé)

Un dump contient vos données en clair. Faites-le passer par `gpg` (ou `age`) pour que le fichier soit chiffré sur le disque. Une phrase secrète vous sera demandée :

```bash
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld | gpg -c --cipher-algo AES256 -o otw-backup-$(date +%F).sql.gpg
```

## Notes de sécurité

- Les **clés API et identifiants des fournisseurs** (flux d'actualités, fournisseurs de données de marché) sont déjà chiffrés au repos avec `OTW_SECRET_KEY`, ils n'apparaissent donc que sous forme chiffrée dans le dump.
- Sauvegardez **`OTW_SECRET_KEY`** (depuis `deploy/.env`) **séparément**, pas dans le même dump, sinon ces secrets chiffrés ne pourront pas être restaurés.
- Le dump inclut des **tokens de session** actifs. Traitez le fichier comme un secret, ou supprimez la table `sessions` après la restauration et reconnectez-vous.
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

Dans une base neuve et vide (une pile tout juste créée) :

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld < otw-backup-2026-07-06.sql
```

Depuis une sauvegarde chiffrée :

```bash
gpg -d otw-backup-2026-07-06.sql.gpg | \
  docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld
```

Assurez-vous que la pile restaurée utilise le **même `OTW_SECRET_KEY`** qu'au moment de la sauvegarde, sinon les identifiants de fournisseurs stockés seront illisibles.
