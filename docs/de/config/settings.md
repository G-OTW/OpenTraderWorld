# Einstellungen im Überblick

Alles unter dem Eintrag **Einstellungen** des Modulwechslers, Abschnitt für Abschnitt.

## Konto

Ändere deinen Benutzernamen oder dein Passwort. Zum Speichern von Änderungen ist dein aktuelles Passwort nötig, und eine Passwortänderung **meldet dich von allen Sitzungen ab**.

Ein neues Passwort muss mindestens 12 Zeichen lang sein und wird abgelehnt, wenn es in öffentlichen Leak-Listen vorkommt. Siehe [Passwortregeln](/de/config/security#password).

## Sicherheit

Zwei-Faktor-Authentifizierung, die Browser, die aktuell an deinem Konto angemeldet sind, und wie lange eine einzelne Anfrage laufen darf. Ausführlich in [Kontosicherheit](/de/config/security).

## Standardwerte

- **Sprache**: gilt sofort für die ganze App (en, fr, de, es, it, pt, zh).
- **Standardwährung** und **Zeitzone**: die Startwerte, die Module für neue Einträge und die Anzeige verwenden.

## Darstellung

Die **Akzentfarbe** der App, die für primäre Schaltflächen, aktive Zustände, Links und Chart-Hervorhebungen verwendet wird. Wähle eine voreingestellte Farbe oder eine beliebige Farbe aus der Auswahl; sie gilt live in der ganzen App und wird für jede Sitzung gespeichert. *Zurücksetzen* stellt den Standard des Themes wieder her.

## Netzwerk

Wer die App erreichen kann: localhost, LAN, LAN + HTTPS oder öffentlich. Ausführlich in [Netzwerk & Fernzugriff](/de/config/network).

## Tresor {#vault}

Ein zentraler Ort für die API-Schlüssel und Geheimnisse, die die App in deinem Auftrag nutzt, statt denselben Schlüssel in jedes Modul einzufügen, das ihn braucht.

Ein **Tresor** steht für einen externen Dienst (z. B. *Binance*) und enthält benannte **Schlüssel**: `apikey`, `secretkey` und so weiter. Lege so viele Tresore und Schlüssel an, wie du brauchst; Module **stecken einen Schlüssel dann per Verweis ein**, über eine gemeinsame Auswahl, überall dort, wo ein Geheimnis verlangt wird (Anbieter-Konnektoren, Feed-Zugangsdaten…).

- **Nur schreibbare Werte.** Ein Geheimnis wird beim Speichern versiegelt und lässt sich nie wieder anzeigen, nur ersetzen oder löschen. Die *Namen* der Schlüssel bleiben sichtbar. Alles wird im Ruhezustand mit dem Hauptschlüssel der App verschlüsselt.
- **Erst abstecken, dann löschen.** Das Löschen eines Tresors oder Schlüssels, der noch in einem Modul eingesteckt ist, wird blockiert; entferne zuerst den Verweis dort. Jeder Tresor zeigt, wie viele Verbindungen ihn nutzen.
- **Anfrageverfolgung** ist optional und gilt **pro Tresor, nicht pro Schlüssel**: Alle Schlüssel eines Tresors zählen auf denselben Zähler. Das Limit dient nur der Information (beobachten und anzeigen); nichts wird je gedrosselt. Es speist dieselbe Ansicht wie [API-Rate](#api-rate).

Auch Geheimnisse von News-Feeds akzeptieren Inline-Platzhalter <code v-pre>{{vault.item}}</code>, die der Scheduler beim Abruf auflöst. Siehe [News](/de/modules/news-research#news).

## Module

Module installieren und trennen. Alles wird mit der App ausgeliefert: Das Installieren macht ein Modul nur im Wechsler und im Dashboard verfügbar; es wird nichts heruntergeladen. Das Trennen blendet es aus und macht es unzugänglich; setze den Haken bei *auch Daten löschen*, um auch seine gespeicherten Daten zu löschen (endgültig).

## Daten verwalten

Speichernutzung je Modul (Tabellen, Zeilen, Größe) mit der Gesamtgröße der Datenbank und eine Aktion **Löschen**, um die Daten eines Moduls endgültig zu löschen (zur Bestätigung den Namen eintippen). Das Löschen lässt sich nicht rückgängig machen.

## Versionen {#versioning}

Zwei Schalter: **Editor-Dateien** und **Strategien**, jeweils mit der Anzahl gespeicherter Versionen und ihrer Größe. Beim Einschalten wirst du gewarnt, dass jede Version eine vollständige Kopie ist und die Datenbank mit jeder wächst (Bilder und Videos werden nie dupliziert). Beim Ausschalten wirst du gefragt, ob die Versionen behalten (verborgen, bis du wieder einschaltest) oder alle gelöscht werden sollen. Ist ein Bereich eingeschaltet, wird die Versionierung pro Datei oder Strategie über ihr Verlaufsmenü aktiviert: siehe [Editor](/de/modules/productivity#editor) und [Backtest](/de/modules/market-data#strategies-and-custom-indicators).

## Sicherung & Wiederherstellung

Zwei Tabs, jeweils mit einer Seite **Sicherung** und einer Seite **Wiederherstellung**:

- **Vollständig**: kopierfertige `pg_dump`- und `psql`-Befehle für dein Deployment, auch verschlüsselte Varianten, plus der Status der automatischen Sicherung auf Instanzen, die eine betreiben.
- **Teilweise**: wähle die gewünschten Module, lade sie als ein Zip herunter und lade dieses Zip hier oder auf einer anderen Instanz wieder ein. Beide Seiten werden vorher gezählt: was du entnimmst, je Modul und Tabelle, und was eine Datei enthält, die du lädst, im Vergleich zu dem, was schon hier ist.

Siehe [Sicherung & Wiederherstellung](/de/guide/backup-restore).

## App aktualisieren

Zeigt die aktuelle Version, prüft GitHub auf eine neuere und listet die Update-Befehle, die auf dem Host auszuführen sind. Siehe [Aktualisieren](/de/guide/updating).

## Protokolle

Der eigene Log-Speicher der App, durchsuchbar nach Meldung/Target. Die **Erfassungsstufe** legt den minimalen Schweregrad fest, der im Speicher abgelegt wird (wirkt sofort). Niedrigere Stufen erfassen mehr Details und brauchen mehr Platz. Hier kannst du gespeicherte Logs löschen.

## API-Rate {#api-rate}

Ein Dashboard der ausgehenden Aufrufe an externe Datenanbieter (Marktdaten, FX, Kurse, Feeds), gezählt pro UTC-Tag: Anfragezahlen je Anbieter, Fehler, Rate-Limit-Antworten, veröffentlichte Limits, wo bekannt, und eine Liste der letzten Rate-Limit-Treffer. Sie existiert, damit du siehst, wie nah du den Limits des kostenlosen Kontingents eines Anbieters bist.

**Diese Seite drosselt nie etwas**: Sie beobachtet nur. Die einzige Stelle, an der ein Limit tatsächlich durchgesetzt wird, ist das [eigene Anfragelimit eines Konnektors](/de/config/connectors#request-limits) bei den Abrufen auf Anforderung im Chart; überall sonst informiert und warnt ein Limit, und der Anbieter bleibt derjenige, der Nein sagt.

## Daten-Konnektoren

Die gemeinsame Liste der Marktdaten-Anbieterkonten, die von Historical Data, Visualization, Watchlists und dem Trading Journal genutzt werden: Zugangsdaten, Anfragelimits und welche Module jeden nutzen dürfen. Behandelt in [Daten-Konnektoren](/de/config/connectors). Dieselbe Ansicht gibt es auch eigenständig unter **/connectors** und über die Konnektor-Schaltfläche in jedem Datenmodul.

## Broker {#brokers}

Die gemeinsame Liste der **schreibgeschützten Broker-Konten**, die vom Trading Journal, den Portfolios, Visualization und dem Tax Calculator genutzt werden: Zugangsdaten, Einstellungen je Broker und welche Module jeden nutzen dürfen. Behandelt in [Broker-Konten](/de/config/brokers). Nichts hier kann eine Order aufgeben, ändern oder stornieren.

## Benachrichtigungen {#notifications}

Die gemeinsame Liste der **Benachrichtigungskanäle**: wohin die App pushen darf, einmal angelegt und von jedem benachrichtigenden Modul wiederverwendet.

Ein Kanal ist ein Ziel, das **dir gehört**. Jeder enthält ein Geheimnis, hier eingetippt oder aus dem [Tresor](#vault) eingesteckt, beim Speichern versiegelt und nie wieder angezeigt.

| Kanal | Was du mitbringst | Geheimnis | Weitere Felder |
|---|---|---|---|
| **E-Mail** | dein eigener SMTP-Server | Passwort | Host, Port (587 STARTTLS, 465 TLS), Absender, Empfänger, Benutzername |
| **Telegram** | ein BotFather-Bot | Bot-Token | Chat-ID |
| **Slack** | ein Incoming Webhook | die Webhook-URL | keine |
| **Discord** | ein Kanal-Webhook | die Webhook-URL | keine |

Alle vier sind für den Host kostenlos: Du bringst das Konto mit, die App bringt nichts mit, wofür man sich anmelden müsste. Einige **nicht geheime** Felder nehmen ebenfalls einen Tresor-Eintrag an, etwa die Telegram-**Chat-ID**, sodass ein Kanal eingerichtet werden kann, ohne dass diese ID im Klartext in der Konfiguration steht.

Die Module, denen ein Kanal gewährt werden kann:

| Modul | Was es pusht |
|---|---|
| **RemindMe** | eine ausgelöste Erinnerung |
| **Watchlists** | ein Kursalarm |
| **Mailbox** | ein Mail-Konto, das Aufmerksamkeit braucht |
| **Webhooks** | eine eingehende Nutzlast, an ein Modul umgeleitet |
| **Historical Data** | eine lange Pause und das Ende eines Download-Batches |
| **Visualization** | ein ausgelöster Chart-Alarm |
| **Journal** | die Marktdaten-Anreicherung eines neuen Trades |
| **Backtest** | ein Paper-Trading-Fill oder seine gruppierte Zusammenfassung |
| **Portfolio Tracker** | vorab gewährbar, bevor er etwas sendet; er pusht heute nichts |
| **Automator** | was immer ein `notify`-Block sendet |

- **Freigaben, pro Modul.** Jeder Kanal nennt die Module, die an ihn senden dürfen, oder *alle*. Die Prüfung läuft serverseitig: Ein Modul, dem nie ein Kanal gewährt wurde, kann ihn nicht erreichen, und das Geheimnis dieses Kanals wird für es nicht einmal entschlüsselt.
- **Ein Schalter pro Kanal.** Das Deaktivieren eines Kanals schaltet ihn überall stumm, ohne ihn oder seine Zugangsdaten zu löschen.
- **Testsendung**, bevor du dich auf einen verlässt.

Dieselbe Ansicht öffnet sich aus jedem benachrichtigenden Modul heraus, sodass ein Kanal im Moment hinzugefügt werden kann, ohne die aktuelle Seite zu verlassen. Sie fehlt bewusst im [MCP](#mcp)-Katalog: Kein Agent kann einen Kanal anlegen oder eine Freigabe erweitern.

## MCP {#mcp}

KI-Agenten die App über ein kontrolliertes Gateway nutzen lassen. Behandelt in [KI-Agenten (MCP)](/de/config/ai-agents).

## Externe Steuerung {#external-control}

Die App über einen Chat-Kanal steuern (Telegram, Slack, Discord). Behandelt in [Externe Steuerung (Chat)](/de/config/external-control).

## Sprache & Tastenkürzel {#voice}

Push-to-talk-Befehle und Diktat: die Spracherkennungs-Engine, die beiden Tastenkürzel und deine Sprachbefehle. Behandelt in [Sprachsteuerung](/de/config/voice). Das Mikrofon funktioniert nur über HTTPS oder auf localhost.

## Danksagungen

Die Datenquellen und Upstream-Projekte, die jedes Modul nutzen kann, auch Anbieter, die du nicht konfiguriert hast.

## Über

Version, Projektlinks und Teilen-Schaltflächen.
