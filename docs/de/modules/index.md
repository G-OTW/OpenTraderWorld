# Modulübersicht

OpenTraderWorld besteht aus **Modulen**, Funktionspaketen, die du einzeln unter **Einstellungen → Module** einschaltest. Alles wird mit der App ausgeliefert; ein Modul zu installieren lässt es im Modulwechsler (oben links) und im Dashboard erscheinen. Trenne ein Modul, um es wieder auszublenden (seine Daten bleiben erhalten, es sei denn, du löschst sie auch).

[Dashboard, Suche und Benachrichtigungen](/de/modules/dashboard) liegen über allen und sind immer da.

## Abhängigkeiten

Einige Module bauen auf dem Datensatzkatalog von **Historical Data** auf und brauchen es installiert:

```
Historical Data ──▶ Historical Data Visualization
                ──▶ Backtest
                ──▶ Quant Tools
```

Alles andere ist unabhängig, wobei sich manche Module integrieren, wenn beide installiert sind (z. B. kann der Tax Calculator den PnL aus dem Trading Journal importieren; MyWealth kann Bestände des Portfolio Tracker importieren; der Calendar kann ToDos, Ziele und Erinnerungen anzeigen).

Historical Data, Visualization, Watchlists, Fundamentals und das Trading Journal teilen sich außerdem eine Liste von **[Daten-Konnektoren](/de/config/connectors)**: Ein Anbieterkonto wird einmal angelegt und den Modulen gewährt, die es nutzen dürfen.

## Alle Module

### Trading

| Modul | Was es tut |
|---|---|
| [Trading Journal](/de/modules/journal) | Trade-Log mit Vorlagen, Gebührenmodellen, Mehrwährungs-FX und Performance-Statistiken. |
| [Trading Routines](/de/modules/productivity#routines) | Wiederkehrende Sitzungs-Checklisten: Vorbereitung vor Marktöffnung, Disziplin während der Sitzung, Review nach Marktschluss. |
| [Mindset](/de/modules/productivity#mindset) | Tägliche Check-ins zu Stimmung und Disziplin mit Trends. |

### Marktdaten & Analyse

| Modul | Was es tut |
|---|---|
| [Historical Data](/de/modules/market-data#histdata) | OHLCV-Historie von mehreren Anbietern in lokale Datensätze laden. |
| [Historical Data Visualization](/de/modules/market-data#histviz) | Ein Arbeitsbereich mit Candle-/OHLC-/Linien-/Renko-Charts mit Indikatoren, Zeichnungen, Vergleichen und serverseitig überwachten Alarmen, live oder auf Abruf, für jedes Instrument, das ein Konnektor bedient. |
| [Backtest](/de/modules/market-data#backtest) | Regelbasierter Strategie-Backtester mit Sizing, Kosten und vollständigen Statistiken. |
| [Quant Tools](/de/modules/market-data#quant) | Risiko, Statistik, Volatilität und Regimes eines Datensatzes; Paare, Baskets und Faktorregression; Overfitting-Tests an gespeicherten Backtests; Sizing, Rechner, Futures-Kurven und Optionsvolatilitätsflächen. |
| [Fundamentals](/de/modules/fundamentals) | Makroreihen, Unternehmensabschlüsse, SEC-Filings, Transkripte, ETFs, ein Marktkalender und Alternativdaten aus Primärquellen und den Aggregatoren, die du verbindest. |

### Portfolios & Geld

| Modul | Was es tut |
|---|---|
| [Watchlists](/de/modules/portfolio#watchlists) | Symbol-Watchlists mit Live-Kursen, Tagesänderungen, Sparklines und Notizen. |
| [Portfolio Tracker](/de/modules/portfolio#portfolios) | Live-Wert, Cash- und Ertrags-Ledger, Performance im Verhältnis zum eingegangenen Risiko, Allokationsdrift und Stresstests. |
| [MyWealth](/de/modules/portfolio#wealth) | Nettovermögen über alles, was du besitzt und schuldest, mit live gelesenen statt kopierten Portfolios. |
| [Managers' Portfolios](/de/modules/portfolio#mportfolios) | 13F-Bestände von Superinvestoren, durchsuchbar und als Snapshot speicherbar. |
| [Tax Calculator](/de/modules/portfolio#taxcalc) | Steuerschätzungen für Trading und Investieren aus Länder-Vorlagen. |
| [Subscriptions](/de/modules/portfolio#subscriptions) | Wiederkehrende Abos und Ausgabenübersicht. |

### News & Recherche

| Modul | Was es tut |
|---|---|
| [News](/de/modules/news-research#news) | RSS- und JSON-API-News-Aggregator mit Dashboards, die per Polling aktualisiert werden. |
| [Mailbox](/de/modules/news-research#mailbox) | Newsletter, Marktnachrichten und Broker-Mails, aus deiner eigenen IMAP-Mailbox gelesen, tracker-frei. |
| [Economic Calendar](/de/modules/news-research#economics) | Anstehende Makro-Ereignisse. |
| [FinanceDatabase](/de/modules/news-research#findb) | Über 300.000 Instrumente lokal durchsuchen; Favoriten in Ordnern organisieren. |
| [Resources](/de/modules/news-research#resources) | Lesezeichen-Bibliothek für Bücher, Links und Referenzen. |
| [Community Docs](/de/modules/news-research#community-docs) | Von der Community geschriebene Anleitungen, synchronisiert und offline lesbar. |

### Notizen & Organisation

| Modul | Was es tut |
|---|---|
| [Editor](/de/modules/productivity#editor) | Rich-Dokumenteditor mit Ordnern und Tabellen-/Kanban-/Galerie-Datenbanken. |
| [ToDo](/de/modules/productivity#todos) | Aufgabenliste mit Fälligkeitsdaten und Kategorien. |
| [Goals](/de/modules/productivity#goals) | Ziele mit Kennzahlenverfolgung und Fristen. |
| [Calendar](/de/modules/productivity#calendar) | Persönlicher Terminkalender; blendet Erinnerungen, ToDos und Ziele ein. |
| [RemindMe](/de/modules/productivity#remindme) | Erinnerungen mit In-App-Benachrichtigungen und E-Mail-/Telegram-/Slack-/Discord-Kanälen. |
| [Time Tracker](/de/modules/productivity#time) | Projekt-Timer mit Budgets und Stundensatz-Wert. |
| [Prompt Store](/de/modules/productivity#prompt-store) | Durchsuchbare Bibliothek wiederverwendbarer KI-Prompts, mit Tags, Bewertung und Versionen. |
| [Webhooks](/de/modules/productivity#webhooks) | Private eingehende URLs, die externe Alarme in Benachrichtigungen verwandeln. |
| [Automator](/de/modules/automator) | Workflows über deine eigene API und die Außenwelt, von Hand oder nach Zeitplan. |

### KI

| Modul | Was es tut |
|---|---|
| [Agent](/de/modules/agent) | Integrierter KI-Chat-Assistent (eigener Anbieter), der über MCP auch auf deine Daten zugreifen kann, mit Gedächtnis, Skills und externen MCP-Servern. |
