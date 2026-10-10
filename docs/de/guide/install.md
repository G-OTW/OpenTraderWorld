# Installation

Hoste OpenTraderWorld selbst, auf deinem eigenen Rechner oder Server. Dauert etwa 5 Minuten.

::: info Nur containerisiert (vorerst)
OpenTraderWorld läuft als Docker-Compose-Stack, dem einzigen unterstützten Deployment. Eine native Installation ist möglich, wird aber nicht empfohlen: Docker hält die Installation unaufdringlich (alles liegt in Containern und Volumes) und schnell neu aufgebaut. Warum, und die Installationsschritte je Betriebssystem, siehe [Docker besorgen](/de/guide/docker).
:::

## Voraussetzungen

- **Docker** mit Docker Compose. Noch nicht vorhanden? [Docker besorgen](/de/guide/docker) behandelt macOS, Windows und Linux in wenigen Befehlen.
- Linux, macOS oder Windows.
- Ein freier Port (standardmäßig **5454**; Ports **80** + **443** für die HTTPS-Modi). Du kannst ihn bei der Einrichtung ändern.

Prüfe, ob Docker bereit ist:

```bash
docker --version
docker compose version
```

## Installation mit einem Befehl (empfohlen)

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

Das Installationsprogramm prüft, ob Docker bereit ist, lädt die Deploy-Dateien (nur das Verzeichnis `deploy/`, kein Quellcode, keine Toolchain) nach `./opentraderworld` und übergibt dann an die geführte Einrichtung unten, die die **vorgefertigten Images** von Docker Hub zieht.

Optionen kommen nach `bash -s --`:

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash -s -- --dir ~/otw
```

| Option | Standard | Hinweise |
|---|---|---|
| `--dir <path>` | `./opentraderworld` | Installationsverzeichnis. Lehnt ein nicht leeres Verzeichnis oder eine bestehende Installation ab. |
| `--ref <ref>` | `master` | Zu installierender Branch oder Tag. |
| `--build` | aus | Den vollständigen Quellcode klonen und die Images lokal bauen, statt sie zu ziehen (braucht `git` und die Toolchain). |

## Aus einem Git-Klon (Alternative)

```bash
git clone https://github.com/G-OTW/OpenTraderWorld.git
cd OpenTraderWorld/deploy
./setup.sh
```

## Die geführte Einrichtung

Beide Wege oben führen `deploy/setup.sh` aus. Es stellt ein paar Fragen, erzeugt starke Geheimnisse, schreibt die Konfiguration und startet alles.

Abgefragt wird:

| Frage | Standard | Hinweise |
|---|---|---|
| **Netzwerkmodus** | `1` (localhost) | `1` nur dieser Rechner · `2` LAN über unverschlüsseltes HTTP · `3` LAN über HTTPS mit echtem Zertifikat · `4` öffentliches Internet unter deiner eigenen Domain. Später änderbar, siehe [Netzwerk & Fernzugriff](/de/config/network). |
| **Admin-Benutzername** | `admin` | Das Admin-Konto wird für dich angelegt; ein starkes Passwort wird erzeugt und **einmalig** angezeigt. |
| **HTTP-Port** | `5454` | Nur Modi 1 und 2; die Modi 3 und 4 nutzen 80 + 443. |
| **DuckDNS-Domain / Token / LAN-IP** | keine | Nur Modus 3. |
| **Öffentliche Domain** | keine | Nur Modus 4, und sie muss bereits auf diesen Server zeigen. |
| **Log-Level** | `info` | `trace` / `debug` / `info` / `warn` / `error`. |

Datenbank- und Sitzungs-**Geheimnisse werden automatisch erzeugt**, du tippst sie also nie ein. Sie werden nach `deploy/.env` geschrieben (Dateirechte `600`, nie in Git eingecheckt).

Lass das Skript am Ende den Stack starten: Es wartet auf die API, **legt dein Admin-Konto an** und gibt das erzeugte Passwort **einmalig** aus, kopiere es also, bevor du das Terminal schließt.

Standardmäßig **zieht die Einrichtung die vorgefertigten Images** von Docker Hub: keine Rust- oder Node-Toolchain, erster Start in ein paar Minuten. Mit `./setup.sh --build` baust du stattdessen die drei Dienste aus dem Quellcode (Entwicklung, lokale Änderungen).

::: tip Server ohne Oberfläche
Der Admin wird beim ersten Start vom Core selbst angelegt (aus `deploy/.env`), du brauchst also keinen Browser auf dem Server. Notiere das ausgegebene Passwort und melde dich von einem beliebigen Rechner an, der die App erreicht.
:::

::: warning Neuinstallation über vorhandene Daten
Wenn Docker-Volumes einer früheren Installation existieren, bietet die Einrichtung an, sie zu löschen. Die Vorgabe ist überall **Nein**: Gelöscht wird (und die bisherige Datenbank geht verloren) nur bei einem ausdrücklichen `y`. Neue Geheimnisse über einem alten Datenbank-Volume können nicht funktionieren, daher bricht die Einrichtung bei einer Ablehnung ab, statt einen defekten Stack zu starten.
:::

## Manuelle Installation (Alternative)

Wenn du lieber von Hand konfigurierst:

```bash
cd deploy
cp .env.example .env
```

Bearbeite `.env` und setze mindestens:

- `POSTGRES_PASSWORD`: ein starkes Passwort
- `DATABASE_URL`: muss dasselbe Passwort enthalten, z. B. `postgres://otw:YOUR_PASSWORD@postgres:5432/opentraderworld`
- `SESSION_SECRET`: eine lange Zufallszeichenfolge

Starte dann den Stack. Standardmäßig **zieht dies die vorgefertigten Images** von Docker Hub (keine Rust- oder Node-Toolchain nötig):

```bash
docker compose -f docker-compose.yml -f docker-compose.images.yml \
  --env-file .env --env-file network.env up -d
```

::: details Stattdessen aus dem Quellcode bauen
Für die Entwicklung oder um lokale Änderungen auszuführen, lässt du die Images-Überschreibung weg und baust die drei Dienste selbst (braucht die Toolchain; der Rust-Build ist langsam):

```bash
docker compose --env-file .env --env-file network.env up --build -d
```
:::

## Den Admin anlegen (nur manuelle Installationen)

Wenn du `./setup.sh` genutzt und den Stack davon starten lassen hast, **existiert dein Admin bereits**, du kannst also weiterspringen.

Öffne andernfalls die App im Browser (standardmäßig `http://localhost:5454`). Beim ersten Besuch erkennt OpenTraderWorld, dass es noch keinen Admin gibt, und zeigt den **Einrichtungsassistenten**: Wähle Benutzername und Passwort (mindestens 8 Zeichen), sende ab, und du landest im Dashboard. Passwörter werden mit argon2 gehasht gespeichert.

::: details Den Admin über die CLI anlegen (ohne Oberfläche, ohne Browser)
Rufe den Erststart-Endpunkt von innerhalb des Stacks auf: Das funktioniert unabhängig von deinem Bind-Interface oder TLS-Modus und wird abgelehnt (HTTP 409), wenn bereits ein Admin existiert:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T caddy \
  wget -qO- --header=Content-Type:application/json \
  --post-data='{"username":"admin","password":"CHOOSE-A-STRONG-ONE"}' \
  http://core:8080/api/setup
```
:::

## Prüfen, ob es läuft

- App: `http://localhost:5454` (oder dein gewählter Port bzw. deine Domain)
- Health-Check: `http://localhost:5454/api/health` → `{"status":"ok","service":"otw-core",...}`

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## Täglicher Betrieb

Führe diese Befehle in `deploy/` aus:

| Aktion | Befehl |
|---|---|
| Starten | `docker compose up -d` |
| Stoppen | `docker compose down` |
| Logs ansehen | `docker compose logs -f` |
| Neuere Images ziehen | `docker compose -f docker-compose.yml -f docker-compose.images.yml pull && docker compose up -d` |
| Nach Code-Änderungen neu bauen (Build aus dem Quellcode) | `docker compose up --build -d` |
| Stoppen **und alle Daten löschen** | `docker compose down -v` |

Deine Daten liegen in benannten Docker-Volumes und **bleiben** über `up`/`down` hinweg erhalten. Sie werden nur mit `down -v` gelöscht.

## Nächste Schritte

- [Erste Schritte nach der Installation](/de/guide/first-steps): anmelden, Standardwerte setzen, Module installieren.
- [Netzwerk & Fernzugriff](/de/config/network): die App von anderen Geräten erreichen, LAN-HTTPS, öffentliche Freigabe.
- Etwas stimmt nicht? Siehe [Fehlerbehebung](/de/guide/troubleshooting).
