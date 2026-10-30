# Sicherung & Wiederherstellung

Alles liegt in einer PostgreSQL-Datenbank, ein Backup ist also ein einziger `pg_dump`. **Einstellungen → Sicherung & Wiederherstellung** in der App zeigt dieselben Befehle. Führe sie auf dem Host aus, auf dem der Stack läuft, im Installationsverzeichnis (dem, das `deploy/` enthält); sie nutzen den vorhandenen Postgres-Container, ohne zusätzlichen Zugriff.

Der Bereich hat zwei Tabs, jeweils aufgeteilt in **Sicherung** und **Wiederherstellung**:

- **Vollständig**: die ganze Datenbank, auf dem Host erstellt, für den Fall, dass die Maschine ausfällt.
- **Teilweise**: die Module, die du ankreuzt, als ein Zip, zum Umziehen oder für eine lesbare Kopie.

## Vollständige Sicherung

Einfacher Dump:

```bash
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner > otw-backup-$(date +%F).sql
```

### Verschlüsseln (empfohlen)

Ein Dump enthält deine Daten im Klartext. Leite ihn durch `gpg` (oder `age`), damit die Datei auf dem Datenträger verschlüsselt ist. Du wirst nach einer Passphrase gefragt; `GPG_TTY` und `--pinentry-mode loopback` lassen diese Abfrage dein Terminal erreichen, während der Dump die Eingabe von gpg füllt:

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner \
  | gpg --symmetric --cipher-algo AES256 --pinentry-mode loopback \
  > otw-backup-$(date +%F).sql.gpg
```

### Automatische Sicherung {#automatic-backup}

Wenn ein Sicherungsskript auf dem Host der App berichtet (das [Server-Installationsprogramm](#server-installs) richtet eines ein), öffnet sich der Tab **Vollständig** mit einer Karte **Automatische Sicherung**: wie lange die letzte Sicherung her ist, wohin die Kopie ging, ob sie geprüft wurde, der Zeitplan und die Größe. Sie wird zur Warnung, wenn der letzte Lauf fehlschlug oder nach 48 Stunden ohne Lauf. Ohne ein solches Skript gibt es keine Karte, und die Befehle oben sind alles.

## Sicherheitshinweise

- **API-Schlüssel und Anbieter-Zugangsdaten** (News-Feeds, Marktdatenanbieter) sind im Ruhezustand bereits mit `OTW_SECRET_KEY` verschlüsselt und erscheinen im Dump daher nur als Chiffretext.
- Sichere **`OTW_SECRET_KEY`** (aus `deploy/.env`) **getrennt**, nicht im selben Dump, sonst lassen sich diese verschlüsselten Geheimnisse nicht wiederherstellen.
- Der Dump enthält aktive **Sitzungstokens**. Behandle die Datei als Geheimnis, oder leere nach der Wiederherstellung die Tabelle `sessions` (`DELETE FROM sessions;`) und melde dich neu an. Lösche die Tabelle nicht: Die Anmeldung braucht sie.
- Bewahre das verschlüsselte Backup **außerhalb der Maschine** auf und rotiere ältere Kopien.

## Teilweise (je Modul)

Der Tab **Teilweise** nimmt die Module, die du ankreuzt, und liefert **eine Zip-Datei**, um ein Journal auf eine andere Instanz zu übertragen oder eine lesbare Kopie zu behalten. Er läuft bei laufender App, anders als die vollständige Sicherung.

### Teilweise Sicherung

1. Öffne **Einstellungen → Sicherung & Wiederherstellung → Teilweise → Sicherung**.
2. Kreuze die gewünschten Module an. Jedes zeigt seine Zeilenzahl und Größe, die angekreuzte Menge wird unter der Liste summiert, und *Was die Auswahl enthält* schlüsselt das Tabelle für Tabelle auf. Historische Bars sind zunächst nicht angekreuzt: Sie sind mit Abstand die größte Tabelle und lassen sich bei deinem Anbieter erneut laden.
3. Lass **Gespeicherte Anbieter-Zugangsdaten einschließen** aus, wenn du nicht genau weißt, wofür du sie brauchst. Diese Werte sind mit dem `OTW_SECRET_KEY` dieser Instanz verschlüsselt und anderswo unlesbar.
4. Klicke auf **Ausgewählte Daten herunterladen**. Du erhältst `otw-data-YYYY-MM-DD.zip`.

Im Zip: `manifest.json` (was enthalten ist, welche Version es geschrieben hat) und je Tabelle eine `tables/<name>.jsonl`, ein JSON-Objekt pro Zeile. Jedes Werkzeug kann sie lesen.

### Teilweise Wiederherstellung

1. Öffne auf der Zielinstanz **Einstellungen → Sicherung & Wiederherstellung → Teilweise → Wiederherstellung** und wähle die Datei.
2. Die Datei wird gelesen, sobald du sie wählst, und nichts wird geschrieben: Du siehst die Version, die sie geschrieben hat, und pro Modul und Tabelle, wie viele Zeilen sie enthält im Vergleich zu den aktuell vorhandenen. Eine sehr große Tabelle wird als Schätzung mit `~` markiert. Eine Datei, die diese Instanz ablehnen würde (beschädigt oder aus einer neueren Version), wird schon an dieser Stelle abgelehnt, bevor du dich auf etwas festlegst.
3. Wähle, wie sie auf die vorhandenen Daten treffen soll:
   - **Fehlendes ergänzen** behält alles Vorhandene und fügt nur Zeilen hinzu, die noch nicht da sind. Nichts wird überschrieben.
   - **Ersetzen** löscht die Daten jedes Moduls in der Datei und lädt dann die Version der Datei. Dafür musst du `REPLACE` eintippen, und es speichert zuvor eine Kopie der aktuellen Daten.

   Die Zeile unter der Auswahl übersetzt diese Zahlen in das, was passieren wird. Ersetzen ist exakt: *löscht die N Zeilen hier, setzt die M Zeilen aus der Datei an ihre Stelle*. Zusammenführen kann nur eine Obergrenze nennen, *fügt bis zu M Zeilen hinzu*: Eine Zeile, deren Schlüssel bereits vorhanden ist, wird übersprungen, und nur das Laden selbst weiß, wie viele das sind.
4. Klicke auf **Diese Datei laden**.

Alles geschieht in einer Transaktion: Schlägt irgendein Teil fehl, wird nichts geändert.

::: warning Eine Datei aus einer neueren Version wird abgelehnt
Ein Bundle aus einer neueren Version wird abgelehnt, statt es zu versuchen. Aktualisiere zuerst die Instanz und lade es dann.
:::

Zeilen behalten ihre ursprünglichen Kennungen, und die ganze Datei wird im Speicher gelesen. Eine sehr große Auswahl (typischerweise historische Bars) wird daher mit einem Hinweis auf die `pg_dump`-Sicherung oben abgelehnt. Das ist das richtige Werkzeug für „alles, auch was ich nie ansehe“.

## Vollständige Wiederherstellung

Ein Dump lässt sich nur in eine leere Datenbank zurückspielen. Der Core führt bei jedem Start seine Migrationen aus, daher kollidiert der Dump mit einer Datenbank, auf der er schon gestartet ist (jeder laufende Stack, auch einer, den `setup.sh` gerade angelegt hat). Stoppe den Core, ersetze die Datenbank durch eine leere, spiele den Dump ein und starte den Core wieder:

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

Die Zeile `DROP DATABASE` löscht, was der Stack jetzt enthält: Erstelle vorher einen Dump davon, wenn es wichtig ist. `ON_ERROR_STOP=1` hält beim ersten Fehler an, statt einen halben Dump einzuspielen.

Aus einem verschlüsselten Backup:

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

Achte darauf, dass der wiederhergestellte Stack denselben **`OTW_SECRET_KEY`** nutzt wie beim Erstellen des Backups, sonst sind gespeicherte Anbieter-Zugangsdaten unlesbar.

## Server-Installationen {#server-installs}

Eine mit dem [Server-Installationsprogramm](/de/guide/hosting/when-it-breaks#installer-options) eingerichtete Maschine hat den Befehl `otw`, der all das für dich erledigt:

| Befehl | Was er tut |
|---|---|
| `otw backup` | Erstellt jetzt eine Sicherung in `/var/backups/otw` und prüft, ob die Datei vollständig ist. |
| `otw restore` | Listet die Sicherungen auf der Maschine auf, fragt, welche, und spielt sie zurück, sobald du `RESTORE` eintippst. Vorher legt er eine vollständige Sicherung der aktuellen Daten an, danach startet er den Core neu. |

Das Installationsprogramm plant außerdem jede Nacht gegen 03:20 ein `otw backup` ein, das die Karte [Automatische Sicherung](#automatic-backup) füllt. Eine nächtliche Kopie lässt die beiden Tabellen weg, die sich erneut herunterladen lassen (den Instrumentenkatalog und die historischen Bars); die Kopie vom Sonntag ist vollständig. Die letzten 10 nächtlichen und 2 vollständigen Kopien werden behalten. Sie bleiben auf der Maschine: Kopiere sie woandershin, damit sie einen ausgefallenen Server überstehen.
