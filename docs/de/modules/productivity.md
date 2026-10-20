# Notizen & Organisation

Die Alltagsmodule: Dokumente, Aufgaben, Ziele, Kalender, Erinnerungen, plus einige für die Disziplin eines Traders.

## Editor {#editor}

Ein Rich-Dokumenteditor im Notion-Stil. Dokumente liegen in einem Ordnerbaum; tippe `/` für das Blockmenü.

- **Blöcke**: Überschriften, Listen, To-do-Listen, Zitate, Codeblöcke, Trennlinien, Links, Bilder (hochgeladen oder per URL), Textfarbe, Hervorhebung, Schriftgröße, normale oder volle Breite. Speichert automatisch.
- **Datenbanken**: ein Dokumenttyp mit typisierten Spalten (Text, Auswahl, URL…), darstellbar als **Tabelle**, **Kanban** (gruppiert nach einer Auswahlspalte) oder **Galerie** (mit einer Titelbild-Spalte). Zeilen und Spalten per Ziehen umsortieren.
- **Zur Veröffentlichung einreichen**: ein Dokument mit erhaltener Formatierung, Sprache, Kategorien und optionaler Autorennennung in die Prüfwarteschlange der [Community-Docs](/de/modules/news-research#community-docs) senden.
- **Versionen** (optional): schalte die Versionierung unter [Einstellungen → Versionen](/de/config/settings#versioning) ein, dann pro Seite oder Datenbank über ihr Verlaufsmenü. **Version speichern** legt einen Snapshot mit Datum und optionaler Notiz ab; öffne eine, um sie schreibgeschützt anzusehen, sie wiederherzustellen (der wiederhergestellte Zustand wird als eine neue Version gespeichert, mit dem Datum der wiederhergestellten Version vermerkt) oder zu löschen. Der Verlauf öffnet sich in einem Fenster mit einer Suche über Notizen und Daten; die Version, die der aktuellen Datei entspricht, ist markiert. Notizen sind auf 500 Zeichen begrenzt. Bilder und Videos werden nicht kopiert: Eine Version zeigt auf dieselben Uploads. Beim Ausschalten der Versionierung für eine Datei wirst du gefragt, ob ihre Versionen behalten oder gelöscht werden sollen. Das Löschen einer Datei löscht nach einer Bestätigung auch ihre Versionen. Agenten mit Schreibzugriff auf den Editor können über [MCP](/de/config/ai-agents) Versionen speichern, wiederherstellen und löschen.

## ToDo {#todos}

Eine Aufgabenliste, die nicht im Weg steht: Aufgaben mit Fälligkeitsdatum, Uhrzeit, Kategorie und Notizen. Filtere nach offen/erledigt/überfällig, sortiere nach Fälligkeit; Markierungen für überfällig/heute/bald erledigen das Erinnern. Ein Dashboard-Widget zeigt, was offen ist.

## Goals {#goals}

Ziele mit **messbaren Kennzahlen**. Gib jedem Ziel eine Frist, eine Kategorie und eine oder mehrere Kennzahlen mit aktuellem Wert, Zielwert und Punkten. Erhöhe sie mit deinem Fortschritt, und der Erfüllungsgrad des Ziels folgt den Punkten. Filtere offen/erreicht/überfällig; per Ziehen ordnen.

## Calendar {#calendar}

Ein persönlicher Kalender (Jahr/Monat/Woche/Tag) für Termine mit Kategorie, Farbe, Ort und Notizen. Sein Kniff sind **Overlays**: Er kann auch deine **Erinnerungen**, **ToDos mit Fälligkeitsdatum** und **Ziel-Fristen** anzeigen, jeweils einzeln schaltbar: ein Ort, um die Woche zu sehen. Beim Anlegen eines Termins kann auch eine synchronisierte Erinnerung zur Startzeit angelegt werden.

## RemindMe {#remindme}

Erinnerungen, einmalig oder wiederkehrend (mit Startdatum, Enddatum oder maximaler Anzahl), die als **In-App-Benachrichtigungen** mit einem Benachrichtigungseingang auslösen.

- **Verknüpfte Erinnerungen**: hänge eine Erinnerung an einen Eintrag in einem anderen Modul (ein Ziel, die Abrechnung eines Abos, ein Journal-Review…), und sie verlinkt direkt dorthin zurück. Die meisten Module haben eine Schaltfläche *Erinnerung hinzufügen*, die das vorbefüllt.
- **Kanäle**: Zustellung auch per **E-Mail, Telegram, Slack oder Discord**, gewählt aus den gemeinsamen [Benachrichtigungskanälen](/de/config/settings#notifications). Eine Erinnerung listet die Kanäle, die RemindMe gewährt wurden; die Zugangsdaten und Freigaben liegen in den Einstellungen, einmal für die ganze App.

## Webhooks {#webhooks}

Gib jedem externen Dienst eine private URL, um **Alarme per POST in OpenTraderWorld zu senden**: Charting- und Alarmplattformen, Broker-Benachrichtigungen, Uptime-Monitore, Skripte, alles, was eine HTTP-Anfrage auslösen kann. Die Nutzlast wird empfangen und in ein Modul geleitet.

- **Private URL, keine Header**: Jeder Endpunkt trägt ein **256-Bit-Token im URL-Pfad** (`/api/hooks/<token>`), weil viele Alarm-Sender keinen `Authorization`-Header setzen können. Tokens werden **gehasht** gespeichert und bei der Erstellung **einmal** angezeigt; fehlgeschlagene Abfragen werden gedrosselt.
- **Großzügige Nutzlasten**: Sende Klartext oder JSON; der Parser akzeptiert lockere Feldnamen, sodass die meisten Sender ohne besondere Formatierung funktionieren.
- **Routing**: Jeder Endpunkt leitet seine Nutzlast an ein Zielmodul um. Das Ziel in v1 ist **[RemindMe](#remindme)**: Eine eingehende Nutzlast wird zu einer In-App-Benachrichtigung, die an deine aktivierten Kanäle weitergepusht wird (E-Mail/Telegram/Slack/Discord).
- **Zustellungslog**: Die jüngsten Zustellungen pro Endpunkt werden aufbewahrt, damit du bestätigen kannst, dass ein Sender dich erreicht, und siehst, was er gesendet hat.

Verwalte Endpunkte unter **/webhooks**.

::: warning Der Sender muss dich erreichen können
Ein Webhook ist nur nützlich, wenn der sendende Dienst eine Verbindung zu deinem Host öffnen kann. Im Netzwerkmodus `local` (und einfachem LAN) kann das von außen niemand, und die Seite warnt dich, wenn der aktuelle Modus nicht aus dem Internet erreichbar ist. Ändere entweder den Modus unter [Einstellungen → Netzwerk](/de/config/network), oder richte einen Tunnel (z. B. Cloudflare Tunnel) auf den Host und halte die App sonst privat.
:::

## Trading Routines {#routines}

Wiederkehrende **Sitzungs-Checklisten**, fällig an den Wochentagen, die du wählst: Vorbereitung vor Marktöffnung, Disziplin während der Sitzung, Review nach Marktschluss. Hake Punkte pro Tag ab, blättere durch vergangene Tage und beobachte den **14-Tage-Konsistenzstreifen**, um zu sehen, ob du wirklich bei deinem Prozess bleibst. Start-Checklisten sind enthalten.

## Time Tracker {#time}

Projekte mit Start/Stopp-**Timern** (oder manuell hinzugefügten Zeiträumen), optionalen **Zeitbudgets** mit Warnungen bei Überschreitung, geplanten Enddaten und einem **Stundensatz**, um die Zeit zu bewerten. Der Tab **Aufschlüsselung** zeigt erfasste Stunden nach Tag/Woche/Monat im Chart, filterbar nach Projekt und Kategorie. Lief ein Timer weiter, während die App geschlossen war, fragt er, ob diese Zeit behalten oder zurückgesetzt werden soll.

## Mindset {#mindset}

Ein tägliches **Check-in** für die Psyche des Traders. Beantworte vor oder nach der Sitzung ein paar Fragen: Skalen (Fokus, Disziplin), Auswahl (ruhig / ängstlich / FOMO), Freitext. Die Fragen sind **vollständig anpassbar**; ein Startsatz ist enthalten. Die Ansicht **Trends** zeigt deine Antworten über die letzten Check-ins im Chart, und im Verlauf liest du jeden Tag erneut nach.

## Prompt Store {#prompt-store}

Eine Bibliothek für die **KI-Prompts**, die du wiederverwendest: Marktrückblicke, Journaling-Fragen, Recherche-Vorlagen. Prompts erscheinen als Raster aus Vignetten (Name, Tags, zuletzt gespeichert) mit einer Suche über Name, Tags und Inhalt.

- **Tags**: Füge im Editor frei wählbare Tags hinzu; filtere das Raster mit der Tag-Leiste.
- **Bewerten & filtern**: Gib einem Prompt Daumen hoch oder runter und filtere schnell auf eines von beiden.
- **Versionsverlauf**: Jedes Speichern wird aufbewahrt; öffne den **Verlauf** eines Prompts, um eine frühere Revision in der Vorschau zu sehen und darauf **zurückzugehen** (die Wiederherstellung wird als neue Version gespeichert, nichts geht verloren).
- **Duplizieren**: Verzweige einen Prompt, um eine Variante abzuleiten.
