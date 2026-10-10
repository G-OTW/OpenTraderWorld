# Sicherung & Wiederherstellung

Alles liegt in einer PostgreSQL-Datenbank, ein Backup ist also ein einziger `pg_dump`. **Einstellungen → Sicherung & Wiederherstellung** in der App zeigt diese Befehle für dein Deployment vorausgefüllt. Führe sie auf dem Host aus, auf dem der Stack läuft; sie nutzen den vorhandenen Postgres-Container, ohne zusätzlichen Zugriff.

Der Bereich hat zwei Tabs, jeweils aufgeteilt in **Sicherung** und **Wiederherstellung**:

- **Vollständig**: die ganze Datenbank, auf dem Host erstellt, für den Fall, dass die Maschine ausfällt.
- **Teilweise**: die Module, die du ankreuzt, als ein Zip, zum Umziehen oder für eine lesbare Kopie.

## Vollständige Sicherung

Einfacher Dump:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld > otw-backup-$(date +%F).sql
```

### Verschlüsseln (empfohlen)

Ein Dump enthält deine Daten im Klartext. Leite ihn durch `gpg` (oder `age`), damit die Datei auf dem Datenträger verschlüsselt ist. Du wirst nach einer Passphrase gefragt:

```bash
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld | gpg -c --cipher-algo AES256 -o otw-backup-$(date +%F).sql.gpg
```

## Sicherheitshinweise

- **API-Schlüssel und Anbieter-Zugangsdaten** (News-Feeds, Marktdatenanbieter) sind im Ruhezustand bereits mit `OTW_SECRET_KEY` verschlüsselt und erscheinen im Dump daher nur als Chiffretext.
- Sichere **`OTW_SECRET_KEY`** (aus `deploy/.env`) **getrennt**, nicht im selben Dump, sonst lassen sich diese verschlüsselten Geheimnisse nicht wiederherstellen.
- Der Dump enthält aktive **Sitzungstokens**. Behandle die Datei als Geheimnis, oder lösche nach der Wiederherstellung die Tabelle `sessions` und melde dich neu an.
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

In eine frische, leere Datenbank (einen neu erstellten Stack):

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld < otw-backup-2026-07-06.sql
```

Aus einem verschlüsselten Backup:

```bash
gpg -d otw-backup-2026-07-06.sql.gpg | \
  docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld
```

Achte darauf, dass der wiederhergestellte Stack denselben **`OTW_SECRET_KEY`** nutzt wie beim Erstellen des Backups, sonst sind gespeicherte Anbieter-Zugangsdaten unlesbar.
