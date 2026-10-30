# Dashboard & Navigation

Der Startbildschirm der App, plus die zwei Dinge, die über jedem Modul liegen: das Suchfeld und der Benachrichtigungseingang.

## Dashboard-Seiten

Das Dashboard bringt fünf vorgefertigte Seiten mit, eine pro Arbeitsweise: **Investor** (langfristige Bestände), **Trader** (Ausführung und Handelssitzung), **Quant** (Recherche, Backtests und Algo-Arbeit), **Portfolio** (Bestände) und **Utilities** (Automatisierung, Notizen und Werkzeuge). Jede startet mit einem fertigen Satz Widgets. Sie werden einmal angelegt und sind danach gewöhnliche Seiten: Ordne sie an, wie du willst. Nur deine eigenen Seiten lassen sich löschen.

Du legst außerdem **deine eigenen Seiten** mit **Custom +** im Bereich **Arbeitsbereich** der Seitenleiste an, der deine Dashboards auflistet. Jede hat einen Namen, eine optionale Beschreibung und eine kurze **Kennzeichnung**, die in der Favoritenleiste steht. Die **Favoritenleiste** oben hält eine bis fünf Seiten, hinzugefügt oder entfernt mit dem Stern daneben; ihr erster Eintrag ist die Seite, auf der sich das Dashboard öffnet. Ist die Leiste voll, fragt ein neuer Stern, welcher Favorit ersetzt wird.

Nutze sie so, wie sich ein Handelstag gliedert: eine Seite *Morgen* mit dem News-Feed, dem Wirtschaftskalender und der Routine-Checkliste; eine Seite *Positionen* mit dem Portfolio und der Watchlist; eine Seite *Verwaltung* mit ToDos und Timern.

## Ein Layout bearbeiten

**Anpassen** öffnet den Modus **Layout bearbeiten**. Eine Seite ist ein freies Raster aus 12 Spalten: Kacheln laufen von links nach rechts und brechen in die nächste Zeile um, gespeicherte Zeilen gibt es nicht. Im Bearbeitungsmodus kannst du:

- **Modul-Kacheln** (ein Link zu einem Modul, dasselbe Modul darf auf beliebig vielen Seiten erscheinen) oder **Widgets** über die Kachel **Hinzufügen** einfügen; **Widget hinzufügen** in der Kopfzeile funktioniert auch außerhalb des Bearbeitungsmodus;
- eine Kachel an eine beliebige Stelle ziehen: Die Kacheln danach rücken mit und das Raster ordnet sich neu; jede Kachel über die Spaltenbreite **in der Größe ändern**;
- die Höhe einer Zeile setzen (**Kompakt**, **Standard** oder **Hoch**) und die **Konfiguration** eines Widgets öffnen (das Zahnrad an der Kachel), wo die Höhe **Titel** ein Widget zu einem Kopfband über die volle Breite macht, für einen Abschnittstitel mit Freitext;
- einen **Abstandhalter** einfügen: eine Lücke über die volle Breite mit einstellbarer Höhe.

Kacheln sind Links, keine Kopien: Eine von einer Seite zu entfernen, berührt nie das Modul oder seine Daten.

## Widgets

Ein Widget ist eine lebendige, interaktive Vorschau eines Moduls: Es liest und schreibt über die eigene API dieses Moduls, was du im Widget tust, ist also real. Widgets, deren Modul nicht installiert ist, werden schlicht nicht angeboten.

Die meisten Widgets gibt es in mehreren **Varianten**, jede eine andere Sicht auf dieselben Daten: Nettovermögen bietet zum Beispiel den aktuellen Wert, seine Veränderung, einen Trend, Vermögen gegenüber Verbindlichkeiten, eine Aufteilung nach Kategorie, zu prüfende Vermögenswerte und die größten Vermögenswerte. **Widget hinzufügen** zeigt jede Variante als eigene Karte, und **Widget type** in der Konfiguration des Widgets wechselt sie später. Die Tabelle nennt, was jedes Widget über seine Varianten hinweg abdeckt.

| Widget | Was es tut |
|---|---|
| **Freitext** | Eine Notiz oder Überschrift, die du selbst schreibst, Markdown-light. |
| **Notes** | Zuletzt bearbeitete Editor-Dokumente, wie viele du hast, und ein einzeiliges Feld für eine neue Notiz. |
| **News-Feed** | Neueste Einträge eines gewählten Feeds, als Liste oder Raster. |
| **Mailbox** | Die neueste ungelesene Mail, neueste zuerst. |
| **Time Tracker** | Einen Projekt-Timer starten/stoppen, ohne die Seite zu verlassen. |
| **Quick Trade** | Kategorie + Vorlage wählen und das Formular zum Hinzufügen eines Trades öffnen. |
| **Journal analytics** | Netto-PnL, Trefferquote, Profit-Faktor, Equity-Kurve, offene Positionen, Exposure, Verhaltensauswertungen und eine Heatmap des täglichen PnL. |
| **Goals** | Eine kurze Zielliste mit Fortschritt; direkt eines hinzufügen. |
| **ToDo** | Offene Aufgaben, an Ort und Stelle abhakbar. |
| **Trading Routine** | Die heutige Checkliste, an Ort und Stelle abhakbar. |
| **Mindset** | Der Check-in des Tages. |
| **Erinnerung** | Ein schnelles Formular zum Hinzufügen einer Erinnerung. |
| **Reminder stats** | Ungelesene Benachrichtigungen, anstehende und überfällige Erinnerungen und die am häufigsten ausgelöste. |
| **Calendar** | Heute und diese Woche auf einen Blick. |
| **Economic Calendar** | Anstehende Makro-Ereignisse, komprimiert. |
| **Portfolio** | Eine Portfolio-Zusammenfassung mit Live-Wert. |
| **Subscriptions** | Monatliche wiederkehrende Ausgaben, dann was als Nächstes verlängert wird. |
| **Nettovermögen** | Aktuelles Nettovermögen, seine Veränderung über ein von dir gesetztes Fenster und eine Sparkline. |
| **Watchlist** | Live-Kurse für eine gewählte Liste: Preis, 24h- und 7d-Änderung. |
| **Fundamentals** | Makroreihen und Boards, ein Unternehmens-Snapshot, eine Abschlusszeile nach Quartal, Jahr oder TTM, sortierbare Unternehmen, gefilterte Filings, anstehende Earnings und Bewertung gegenüber gespeicherten Peers. Aus gespeicherten Daten gelesen, ohne Anbieterkontingent zu verbrauchen. |
| **Automator** | Laufende Workflows, nächste geplante Läufe, Erfolgsquote pro Workflow, letzte Fehlschläge und bevorzugte Schnellläufe. |
| **Webhooks** | Empfangene Ereignisse, letzte Ereignisse, Fehlerquote pro Endpunkt und verstummte Endpunkte. |
| **Historical data** | Gespeicherte Datensätze, Speichergröße, die Download-Warteschlange, Abdeckungslücken, Aktualität und fehlgeschlagene Downloads. |
| **Backtest** | Bester gespeicherter Lauf, die Bestenliste, die Strategiebibliothek und bevorzugte Schnellläufe. |
| **Finance database** | Favoriten nach Klassifizierung und Ordner, die Katalogaktualisierung und veraltete Favoriten. |
| **Charts** | Gespeicherte Layouts und Listen, aktive Chart-Alarme und die letzte Alarmaktivität. |
| **Managers' portfolios** | Verfolgte Manager, die größten Depots, Bestandsdrift und die größten Positionen nach Veränderung. |
| **Community Docs** | Gespeicherte Referenzen nach Kategorie, neue Einträge und Schnellzugriff. |
| **Quant tools** | Verfügbare Datensätze und Backtests, Single-Asset-Risiko und Drawdown, Korrelation, Saisonalität, realisierte Volatilität gegenüber ihrer historischen Spanne und das geschätzte Marktregime. |
| **Tax calculator** | Geschätzte Steuer des Jahres, der effektive Steuersatz und eine Tabelle im Jahresvergleich. |
| **Prompt Store** | Deine Prompts nach Tag, klicke einen an, um ihn zu kopieren. |
| **Resources** | Lesezeichen aus einer gewählten Kategorie. |
| **Agent** | Den Assistenten fragen: Modell und Werkzeuge wählen, senden, in der Unterhaltung landen. |
| **Agent activity** | Letzte Unterhaltungen und Tokenverbrauch. |

Fundamentals- und Quant-Widgets aktualisieren sich alle fünf Minuten, solange die Seite sichtbar ist. Sie behalten während einer Aktualisierung ihr vorheriges Ergebnis und erklären ein fehlgeschlagenes Update. Fundamentals liest nur gespeicherte Snapshots; fehlende Daten lädst oder aktualisierst du auf der entsprechenden Modulseite.

In den **Widget-Einstellungen** wählst du einen Quant-Datensatz oder einen Basket aus zwei bis zwanzig kompatiblen Datensätzen. Basket-Mitglieder müssen denselben Timeframe teilen; Intraday-Mitglieder müssen außerdem denselben Anbieter teilen. Risiko bietet 90 %, 95 % oder 99 % Konfidenz für historisches VaR, Saisonalität bietet Renditen, Volatilität, Volumen oder Bar-Spanne, und die Karten für Volatilität und Regime legen ihr Fenster oder ihre Zustandsanzahl offen. Jede Analyse zeigt ihre tatsächliche Historie und Stichprobengröße.

Die Fundamentals-Einstellungen lassen dich die Reihen eines Makro-Boards wählen und ordnen, bis zu vier Unternehmenskennzahlen oder Tabellenspalten auswählen, Peer-Unternehmen wählen und Filings und Earnings auf verfolgte Unternehmen filtern. Statement-TTM summiert vier aufeinanderfolgende Quartale und ist für Gewinn- und Verlust- und Cashflow-Zeilen verfügbar; Bilanzwerte bleiben Beobachtungen zum Periodenende. Ein Vorjahresvergleich setzt dieselbe Fiskalperiode im Vorjahr und eine positive Vergleichsbasis voraus.

Fahre über eine Heatmap-Zelle, fokussiere oder tippe sie an, um den Wert und die Stichprobenzahl zu prüfen. Fehlende Zellen sind schraffiert, unterscheidbar von einer gemessenen Null. Schmale Karten zeigen monatliche Saisonalitätszusammenfassungen oder die stärksten Paarkorrelationen. Widget-Links öffnen den passenden Modul-Tab mit der gewählten Reihe, dem Datensatz oder Basket.

Auf Bildschirmen bis 767 px Breite stapeln sich Dashboard-Karten in einer einzigen Spalte, und im Bearbeitungsmodus ersetzen die Knöpfe **Nach oben** / **Nach unten** das Ziehen. Die gespeicherte Anordnung bleibt auf breiteren Bildschirmen erhalten.

## Globale Suche

Das Suchfeld in der oberen Leiste, von überall mit <kbd>⌘K</kbd> / <kbd>Strg+K</kbd> fokussierbar, oder einfach <kbd>/</kbd>, wenn du nicht in einem Feld tippst.

Standardmäßig findet es **Modulnamen**, **Einstellungsbereiche** und Einträge unter **Resources**. Der **Ebenen-Schalter** neben dem Feld erweitert es auf deine Inhalte: Editor-Seiten, Ziele, Kalendereinträge, ToDos, Routinen, Erinnerungen, Prompts, Mail-Betreffzeilen, Community-Docs sowie Unternehmen, Reihen und Dokumente aus Fundamentals.

Zwei Dinge tut es bewusst nicht: Es findet **nur Titel und Namen, nie Inhalte**, und es durchsucht nur Module, die du installiert hast. Ergebnisse kommen nach Typ gruppiert zurück, Präfix-Treffer zuerst; <kbd>↑</kbd>/<kbd>↓</kbd> und <kbd>Enter</kbd> navigieren darin.

## Benachrichtigungen

Die Glocke in der oberen Leiste trägt einen Zähler für Ungelesenes und öffnet den **Benachrichtigungseingang**, in dem [RemindMe](/de/modules/productivity#remindme)-Erinnerungen landen, wenn sie auslösen, zusammen mit Benachrichtigungen anderer Module: Automator-Workflows, eingehende [Webhooks](/de/modules/productivity#webhooks), Hintergrund-Jobs wie Mailbox-Abrufe, Daten-Downloads und Broker-Synchronisierungen sowie Sicherheitsereignisse wie eine Anmeldung von einer Adresse, die dieses Konto noch nie benutzt hat. Eine Benachrichtigung, die auslöst, während du in der App bist, gleitet außerdem als Banner ein.

Die Zustellung per **E-Mail, Telegram, Slack oder Discord** läuft über die gemeinsamen [Benachrichtigungskanäle](/de/config/settings#notifications) in den Einstellungen, wo du auch entscheidest, welche Module an welchen pushen dürfen. Der Eingang selbst ist immer an und braucht keine Einrichtung.
