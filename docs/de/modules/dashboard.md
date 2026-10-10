# Dashboard & Navigation

Der Startbildschirm der App, plus die zwei Dinge, die über jedem Modul liegen: das Suchfeld und der Benachrichtigungseingang.

## Dashboard-Seiten

Das Dashboard öffnet sich auf einer eingebauten Seite **Module**: eine Kachel pro installiertem Modul, automatisch neu aufgebaut, wenn du Module installierst und trennst. Sie wird nie bearbeitet oder gelöscht; sie spiegelt nur, was du hast.

Darüber legst du **deine eigenen Seiten** an. Jede hat einen Namen, eine optionale Beschreibung und eine kurze **Kennzeichnung** auf ihrem Chip. Eine Seite ist der **Standard**: diejenige, auf der sich das Dashboard öffnet und deren Chip zuerst sortiert wird.

Nutze sie so, wie sich ein Handelstag gliedert: eine Seite *Morgen* mit dem News-Feed, dem Wirtschaftskalender und der Routine-Checkliste; eine Seite *Positionen* mit dem Portfolio und der Watchlist; eine Seite *Verwaltung* mit ToDos und Timern.

## Ein Layout bearbeiten

**Layout bearbeiten** macht aus einer Seite ein Raster aus Zeilen über 12 Spalten. Im Bearbeitungsmodus kannst du:

- **Zeilen hinzufügen** und **Modul-Kacheln** (ein Link zu einem Modul, dasselbe Modul darf auf beliebig vielen Seiten erscheinen) oder **Widgets** hineinlegen;
- jede Kachel über die Spaltenbreite **in der Größe ändern** und Kacheln zwischen Zeilen ziehen;
- die **Höhenvorgabe** eines Widgets setzen (kompakt, Standard oder hoch) und seine **Konfiguration** öffnen (das Zahnrad an der Kachel);
- **Abstandszeilen** einfügen, um zwischen Blöcken Luft zu lassen.

Kacheln sind Links, keine Kopien: Eine von einer Seite zu entfernen, berührt nie das Modul oder seine Daten.

## Widgets

Ein Widget ist eine lebendige, interaktive Vorschau eines Moduls: Es liest und schreibt über die eigene API dieses Moduls, was du im Widget tust, ist also real. Widgets, deren Modul nicht installiert ist, werden schlicht nicht angeboten.

| Widget | Was es tut |
|---|---|
| **Freitext** | Eine Notiz oder Überschrift, die du selbst schreibst, Markdown-light. |
| **News-Feed** | Neueste Einträge eines gewählten Feeds, als Liste oder Raster. |
| **Mailbox** | Die neueste ungelesene Mail, neueste zuerst. |
| **Time Tracker** | Einen Projekt-Timer starten/stoppen, ohne die Seite zu verlassen. |
| **Quick Trade** | Kategorie + Vorlage wählen und das Formular zum Hinzufügen eines Trades öffnen. |
| **Goals** | Eine kurze Zielliste mit Fortschritt; direkt eines hinzufügen. |
| **ToDo** | Offene Aufgaben, an Ort und Stelle abhakbar. |
| **Trading Routine** | Die heutige Checkliste, an Ort und Stelle abhakbar. |
| **Mindset** | Der Check-in des Tages. |
| **Erinnerung** | Ein schnelles Formular zum Hinzufügen einer Erinnerung. |
| **Calendar** | Heute und diese Woche auf einen Blick. |
| **Economic Calendar** | Anstehende Makro-Ereignisse, komprimiert. |
| **Portfolio** | Eine Portfolio-Zusammenfassung mit Live-Wert. |
| **Subscriptions** | Monatliche wiederkehrende Ausgaben, dann was als Nächstes verlängert wird. |
| **Nettovermögen** | Aktuelles Nettovermögen, seine Veränderung über ein von dir gesetztes Fenster und eine Sparkline. |
| **Watchlist** | Live-Kurse für eine gewählte Liste: Preis, 24h- und 7d-Änderung. |
| **Fundamentals** | Makroreihen und Boards, ein Unternehmens-Snapshot, eine Abschlusszeile nach Quartal, Jahr oder TTM, sortierbare Unternehmen, gefilterte Filings, anstehende Earnings und Bewertung gegenüber gespeicherten Peers. Aus gespeicherten Daten gelesen, ohne Anbieterkontingent zu verbrauchen. |
| **Quant** | Verfügbare Datensätze und Backtests, Single-Asset-Risiko und Drawdown, Korrelation, Saisonalität, realisierte Volatilität gegenüber ihrer historischen Spanne und das geschätzte Marktregime. |
| **Prompt Store** | Deine Prompts nach Tag, klicke einen an, um ihn zu kopieren. |
| **Resources** | Lesezeichen aus einer gewählten Kategorie. |
| **Agent** | Den Assistenten fragen: Modell und Werkzeuge wählen, senden, in der Unterhaltung landen. |

Fundamentals- und Quant-Widgets aktualisieren sich alle fünf Minuten, solange die Seite sichtbar ist. Sie behalten während einer Aktualisierung ihr vorheriges Ergebnis und erklären ein fehlgeschlagenes Update. Fundamentals liest nur gespeicherte Snapshots; fehlende Daten lädst oder aktualisierst du auf der entsprechenden Modulseite.

In den **Widget-Einstellungen** wählst du einen Quant-Datensatz oder einen Basket aus zwei bis zwanzig kompatiblen Datensätzen. Basket-Mitglieder müssen denselben Timeframe teilen; Intraday-Mitglieder müssen außerdem denselben Anbieter teilen. Risiko bietet 90 %, 95 % oder 99 % Konfidenz für historisches VaR, Saisonalität bietet Renditen, Volatilität, Volumen oder Bar-Spanne, und die Karten für Volatilität und Regime legen ihr Fenster oder ihre Zustandsanzahl offen. Jede Analyse zeigt ihre tatsächliche Historie und Stichprobengröße.

Die Fundamentals-Einstellungen lassen dich die Reihen eines Makro-Boards wählen und ordnen, bis zu vier Unternehmenskennzahlen oder Tabellenspalten auswählen, Peer-Unternehmen wählen und Filings und Earnings auf verfolgte Unternehmen filtern. Statement-TTM summiert vier aufeinanderfolgende Quartale und ist für Gewinn- und Verlust- und Cashflow-Zeilen verfügbar; Bilanzwerte bleiben Beobachtungen zum Periodenende. Ein Vorjahresvergleich setzt dieselbe Fiskalperiode im Vorjahr und eine positive Vergleichsbasis voraus.

Fahre über eine Heatmap-Zelle, fokussiere oder tippe sie an, um den Wert und die Stichprobenzahl zu prüfen. Fehlende Zellen sind schraffiert, unterscheidbar von einer gemessenen Null. Schmale Karten zeigen monatliche Saisonalitätszusammenfassungen oder die stärksten Paarkorrelationen. Widget-Links öffnen den passenden Modul-Tab mit der gewählten Reihe, dem Datensatz oder Basket.

Auf Handy-Bildschirmen bis 480 px Breite stapeln sich Dashboard-Karten in einer einzigen Spalte. Die gespeicherte Anordnung bleibt auf breiteren Bildschirmen und im Layout-Editor erhalten.

## Globale Suche

Das Suchfeld in der oberen Leiste, von überall mit <kbd>⌘K</kbd> / <kbd>Strg+K</kbd> fokussierbar, oder einfach <kbd>/</kbd>, wenn du nicht in einem Feld tippst.

Standardmäßig findet es **Modulnamen**, **Einstellungsbereiche** und Einträge unter **Resources**. Der **Ebenen-Schalter** neben dem Feld erweitert es auf deine Inhalte: Editor-Seiten, Ziele, Kalendereinträge, ToDos, Routinen, Erinnerungen, Prompts und Community-Docs.

Zwei Dinge tut es bewusst nicht: Es findet **nur Titel und Namen, nie Inhalte**, und es durchsucht nur Module, die du installiert hast. Ergebnisse kommen nach Typ gruppiert zurück, Präfix-Treffer zuerst; <kbd>↑</kbd>/<kbd>↓</kbd> und <kbd>Enter</kbd> navigieren darin.

## Benachrichtigungen

Die Glocke in der oberen Leiste trägt einen Zähler für Ungelesenes und öffnet den **Benachrichtigungseingang**, in dem [RemindMe](/de/modules/productivity#remindme)-Erinnerungen landen, wenn sie auslösen, zusammen mit allem, was ein eingehender [Webhook](/de/modules/productivity#webhooks) dorthin umleitet. Eine Benachrichtigung, die auslöst, während du in der App bist, gleitet außerdem als Banner ein.

Die Zustellung per **E-Mail, Telegram, Slack oder Discord** läuft über die gemeinsamen [Benachrichtigungskanäle](/de/config/settings#notifications) in den Einstellungen, wo du auch entscheidest, welche Module an welchen pushen dürfen. Der Eingang selbst ist immer an und braucht keine Einrichtung.
