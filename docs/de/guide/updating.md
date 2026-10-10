# Aktualisieren

OpenTraderWorld zeigt dir unter **Einstellungen → App aktualisieren** an, wenn eine neue Version verfügbar ist (es prüft GitHub). Die App **kann sich bewusst nicht selbst aktualisieren** (sie läuft ohne Shell- oder Docker-Zugriff, um die Angriffsfläche klein zu halten), Updates sind also ein paar Befehle auf dem Host.

## Vor dem Update

1. **Erstelle ein Datenbank-Backup**: siehe [Sicherung & Wiederherstellung](/de/guide/backup-restore).
2. Überfliege die Release Notes auf inkompatible Änderungen.

## Welche Installation hast du?

Sieh in dein Installationsverzeichnis:

- Nur `deploy/` darin, kein `.git`: **Image-Installation** (das Ein-Befehl-Installationsprogramm, oder
  `setup.sh` ohne `--build`). Das ist der Standard.
- `core/`, `frontend/` und ein `.git`: **Build aus dem Quellcode** (`install.sh --build`, oder ein Klon
  plus `./setup.sh --build`).

## Image-Installation

Es wird nichts gebaut und es gibt keinen Git-Checkout, also aktualisiere `deploy/` aus dem Release
(dort sind die neuen Image-Tags festgelegt), ziehe dann die Images und starte neu:

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

Deine Konfiguration bleibt erhalten: `.env`, `network.env` und `dns.env` gehören nicht zum
Release, die Kopie überschreibt sie also nie. Alles andere in `deploy/` wird durch die
neue Version ersetzt, und das ist der Sinn: Lokale Änderungen an `docker-compose.yml` oder `Caddyfile`
gehen verloren, halte sie bei Bedarf als Patch fest.

## Build aus dem Quellcode

Aktualisiere den Checkout und baue dann neu:

```bash
cd /path/to/OpenTraderWorld
git fetch origin
git reset --hard origin/master
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d --build
```

::: warning `git reset --hard` verwenden, nicht `git pull`
Jedes Release wird als frischer Snapshot des Repositorys veröffentlicht, daher meldet `git pull`
„divergent branches“ und schlägt fehl. `git reset --hard origin/master` bringt deinen Checkout
exakt auf den Stand des neuen Releases. Deine Daten und Konfiguration bleiben unberührt: Sie liegen in
Docker-Volumes und in `.env` / `network.env`, die nicht von Git verfolgt werden. Hast du
verfolgte Dateien lokal bearbeitet, lege sie zuerst beiseite (`git stash`).
:::

Um die veröffentlichten Images zu ziehen, statt aus deinem Checkout neu zu bauen, füge
`-f deploy/docker-compose.images.yml` hinzu und nutze `pull` + `up -d` wie bei der Image-Installation.

Das war's schon:

- Container werden mit der neuen Version neu erstellt und neu gestartet.
- **Datenbankmigrationen laufen automatisch** beim ersten Start des neuen Core-Containers.
- Deine Daten bleiben unberührt: Sie liegen in Docker-Volumes, unabhängig von den Images.

Die App ist kurz offline, während die Container neu erstellt werden. Die genauen Befehle (mit deinen konfigurierten Pfaden) zeigt auch **Einstellungen → App aktualisieren**.

## Einmalig: OAuth für KI-Agenten (0.0.16) {#oauth-caddyfile}

Die OAuth-Anmeldung für MCP-Clients braucht, dass die Discovery-Dokumente unter `/.well-known/oauth-*`
den Core erreichen. Das oben beschriebene Verfahren für Image-Installationen und Builds aus dem Quellcode bringen die neue
`deploy/Caddyfile` mit, ebenso Neuinstallationen. Eine mit `otw update` aktualisierte Installation behält ihre
alte Caddyfile: Alles funktioniert außer OAuth, bis du diesen Block direkt vor der Zeile
`# Everything else → static frontend` einfügst:

```
	handle /.well-known/oauth-* {
		header Content-Security-Policy "default-src 'none'; frame-ancestors 'none'"
		reverse_proxy core:8080
	}
```

Erstelle dann Caddy neu (ein Reload genügt nicht: Die meisten Editoren ersetzen die Datei, und der
Container behält die alte):

```bash
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d --force-recreate caddy
```

## Einmalig: das Bootstrap-Passwort stilllegen {#retire-bootstrap-password}

Wenn der Core beim Start Folgendes loggt, richtet er sich an dich:

```
OTW_ADMIN_PASSWORD is still set but the admin account already exists.
```

`setup.sh` legt den Admin beim ersten Start aus `deploy/.env` an und leert dann diese Zeile. Bei
einer Installation, die vor dieser Änderung entstand, steht der Wert noch in der Datei, und in der
Umgebung des Containers, wo `docker inspect` ihn jedem zeigt, der den Docker-Daemon erreicht.
Er gewährt zu diesem Zeitpunkt nichts, er ist nur eine weitere Kopie eines Passworts auf dem Datenträger.

```bash
sed -i.bak 's/^OTW_ADMIN_PASSWORD=.*/OTW_ADMIN_PASSWORD=/' deploy/.env
chmod 600 deploy/.env && rm -f deploy/.env.bak
```

Erstelle dann den Core neu, damit er auch aus der Umgebung verschwindet:

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d core
```

Behalte `OTW_ADMIN_USER`: Ein leeres Passwort macht den ganzen Bootstrap wirkungslos, was du
willst, und die Zeile dokumentiert weiterhin, wie eine Installation ohne Oberfläche ihr erstes Konto anlegt.
