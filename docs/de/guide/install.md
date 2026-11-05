# Installation

Hoste OpenTraderWorld selbst, auf deinem eigenen Rechner oder Server. Dauert etwa 5 Minuten.

::: info Nur containerisiert (vorerst)
OpenTraderWorld läuft als Docker-Compose-Stack, dem einzigen unterstützten Deployment. Eine native Installation ist möglich, wird aber nicht empfohlen: Docker hält die Installation unaufdringlich (alles liegt in Containern und Volumes) und schnell neu aufgebaut. Warum, und die Installationsschritte je Betriebssystem, siehe [Docker besorgen](/de/guide/docker).
:::

## Voraussetzungen

- **Docker** mit Docker Compose. Noch nicht vorhanden? [Docker besorgen](/de/guide/docker) behandelt macOS, Windows und Linux in wenigen Befehlen.
- Linux, macOS oder Windows.
- Ein freier Port (standardmäßig **5454**; Ports **80** + **443** für die HTTPS-Modi). Du kannst ihn bei der Einrichtung ändern. Der localhost-Modus und der LAN-Modus mit unverschlüsseltem HTTP belegen außerdem **8443** auf `127.0.0.1`.

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
| `--yes` | aus | Wird an `setup.sh` weitergegeben: jede Vorgabe übernehmen, wenn kein Terminal zum Fragen da ist. |
| `--wipe-volumes` | aus | Wird an `setup.sh` weitergegeben: die Volumes einer früheren Installation ohne Rückfrage entfernen. **Das löscht die Daten dieser Installation.** |

`OTW_DIR` und `OTW_REF` setzen dieselben Werte wie `--dir` und `--ref` (eine Option hat Vorrang vor ihrer Variablen), und `OTW_REPO` das GitHub-Repository (Standard `G-OTW/OpenTraderWorld`).

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

Außerdem nimmt es diese Optionen an:

| Option | Hinweise |
|---|---|
| `--build` | Die Images aus dem Quellcode bauen, statt sie zu ziehen (siehe unten). |
| `--yes` | Jede Vorgabe übernehmen, wenn kein Terminal zum Fragen da ist. |
| `--wipe-volumes` | Die Volumes einer früheren Installation ohne Rückfrage entfernen. **Das löscht die Daten dieser Installation.** |
| `--non-interactive` | Wie `OTW_NONINTERACTIVE=1`: jede Antwort aus der Umgebung lesen, nie aus einem Terminal. Eine nicht gesetzte Variable nimmt die Vorgabe. |

Die Variablen für den nicht interaktiven Betrieb, eine pro Frage: `NET_CHOICE` (`1` bis `4`), `OTW_HTTP_PORT`, `ADMIN_USER`, `OTW_ADMIN_PASSWORD` (erzeugt, wenn nicht gesetzt), `OTW_LOG`, `DUCK_DOMAIN`, `DUCK_TOKEN` und `LAN_IP` (Modus 3), `OTW_DOMAIN` (Modus 4), sowie `OVERWRITE`, `CLEAN_VOLS`, `WIPE_VOL`, `START_NOW` (`y` oder `N`) für die Bestätigungen.

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
- `OTW_SECRET_KEY`: eine weitere lange Zufallszeichenfolge, der Schlüssel, der gespeicherte Zugangsdaten verschlüsselt. Halte ihn stabil: Wird er später geändert, sind sie unlesbar.

Compose braucht außerdem die beiden Netzwerkdateien, die `setup.sh` geschrieben hätte. Für den localhost-Modus auf Port 5454:

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

Beide werden später aus **Einstellungen → Netzwerk** neu geschrieben, siehe [Netzwerk & Fernzugriff](/de/config/network).

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

Öffne andernfalls die App im Browser (standardmäßig `http://localhost:5454`). Beim ersten Besuch erkennt OpenTraderWorld, dass es noch keinen Admin gibt, und zeigt den **Einrichtungsassistenten**: Wähle Benutzername und Passwort, sende ab, und du landest im Dashboard. Das Passwort braucht mindestens 12 Zeichen; ein wiederholtes Zeichen, eine abgezählte Folge, eines, das den Benutzernamen enthält, oder eines aus einer bekannten Leak-Liste wird abgelehnt. Passwörter werden mit argon2 gehasht gespeichert.

In den HTTPS-Modi (LAN + HTTPS, öffentlich) fragt der Assistent zusätzlich nach einem **Setup-Token**, damit niemand, der nur die Adresse findet, das Konto zuerst beanspruchen kann. Der Core gibt es beim Start in seinem Log aus: `docker compose logs core`.

::: details Den Admin über die CLI anlegen (ohne Oberfläche, ohne Browser)
Rufe den Erststart-Endpunkt von innerhalb des Stacks auf: Das funktioniert unabhängig von deinem Bind-Interface und wird abgelehnt (HTTP 409), wenn bereits ein Admin existiert. In den HTTPS-Modi füge das Setup-Token aus dem Core-Log als weiteren Header hinzu, `--header=X-OTW-Setup-Token:<token>`, sonst antwortet er mit 403 `setup_token_required`:

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
- Health-Check: `http://localhost:5454/api/health` → `{"status":"ok"}` (angemeldet liefert er außerdem die Version, die Laufzeit und den Zustand jedes Dienstes)

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## Täglicher Betrieb

Führe diese Befehle in `deploy/` aus. Jeder Befehl braucht beide Env-Dateien, und eine Image-Installation zusätzlich die Images-Überschreibung, also beginne mit der Zeile, die zu deiner Installation passt:

```bash
# image install (the default)
docker compose -f docker-compose.yml -f docker-compose.images.yml --env-file .env --env-file network.env
# source build
docker compose --env-file .env --env-file network.env
```

| Aktion | Anhängen |
|---|---|
| Starten | `up -d` |
| Stoppen | `down` |
| Logs ansehen | `logs -f` |
| Nach Code-Änderungen neu bauen (Build aus dem Quellcode) | `up --build -d` |
| Stoppen **und alle Daten löschen** | `down -v` |

Für eine neuere Version folge [Aktualisieren](/de/guide/updating): Die Image-Tags sind in `docker-compose.images.yml` festgelegt, ein einfaches `pull` holt also die Version, die du schon betreibst.

Deine Daten liegen in benannten Docker-Volumes und **bleiben** über `up`/`down` hinweg erhalten. Sie werden nur mit `down -v` gelöscht.

## Nächste Schritte

- [Erste Schritte nach der Installation](/de/guide/first-steps): anmelden, Standardwerte setzen, Module installieren.
- [Netzwerk & Fernzugriff](/de/config/network): die App von anderen Geräten erreichen, LAN-HTTPS, öffentliche Freigabe.
- Etwas stimmt nicht? Siehe [Fehlerbehebung](/de/guide/troubleshooting).
