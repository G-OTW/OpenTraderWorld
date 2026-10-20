# Was ist OpenTraderWorld?

> Projektwebsite: **[opentraderworld.com](https://opentraderworld.com)**: Modulrundgang,
> [Live-Demo](https://demo.opentraderworld.com), [Community-Anleitungen](https://opentraderworld.com/docs)
> und [Abstimmung über die Roadmap](https://opentraderworld.com/suggestions).

OpenTraderWorld ist eine **selbst gehostete Webplattform für Trader und Investoren**. Du installierst sie einmal mit Docker auf deinem eigenen Computer oder Server, öffnest sie im Browser und erhältst einen privaten Arbeitsbereich aus Modulen: ein Trading-Journal, historische Marktdaten mit Charting und Backtesting, Portfolio- und Vermögensverfolgung, einen News-Aggregator, Notizen, Checklisten und mehr.

**Kostenlos für alle, privat oder beruflich. Source-available (FSL-1.1-MIT).** Nur weiterverkaufen oder als bezahlten Dienst anbieten darfst du es nicht. Das Leitprinzip: *erst profitabel werden, bevor du einen Cent ausgibst.*

## Warum selbst gehostet?

- **Deine Daten bleiben bei dir.** Trades, Portfolios, Notizen und Journal-Einträge liegen in einer PostgreSQL-Datenbank auf deinem Rechner, nicht auf dem Server eines anderen.
- **Standardmäßig privat.** Nach der Installation lauscht die App nur auf `localhost`. Sie im LAN oder im Internet erreichbar zu machen, ist eine bewusste Entscheidung unter [Einstellungen → Netzwerk](/de/config/network).
- **Kein Abo.** Der Betrieb der Kernwerkzeuge kostet nichts. Einige Module können optional externe Datenanbieter nutzen (viele mit kostenlosem Kontingent), und du bringst deine eigenen API-Schlüssel mit.

## So funktioniert es

Ein `docker compose`-Stack, vier Dienste:

| Dienst | Aufgabe |
|---|---|
| **core** | Rust-API-Server (Axum): gesamte Geschäftslogik, Scheduler, Hintergrundjobs |
| **postgres** | PostgreSQL: der einzige Ort, an dem deine Daten liegen |
| **frontend** | SvelteKit-Single-Page-App, einmalig beim Deployment gebaut |
| **caddy** | Reverse Proxy: liefert die App aus, leitet `/api` weiter, verwaltet HTTPS-Zertifikate |

Die App ist **Einzelnutzer**: ein Admin-Konto, bei der Installation angelegt. Es gibt keinen Mehrmandantenbetrieb, kein Teilen und keine Benutzerverwaltung zu konfigurieren.

Docker ist derzeit das **einzige unterstützte Deployment**: Es hält die Installation unaufdringlich und schnell neu aufgebaut ([warum, und wie du Docker bekommst](/de/guide/docker)). Eine native Installation ist möglich, wird aber nicht empfohlen.

## Die Module

Module sind Funktionspakete, die du unter **Einstellungen → Module** installierst oder abkoppelst. Alles wird mit der App ausgeliefert, und das Installieren eines Moduls schaltet es nur ein. Highlights:

- **[Trading-Journal](/de/modules/journal)**: Trades mit Vorlagen, Gebührenmodellen, Mehrwährungs-PnL und vollständigen Performance-Statistiken erfassen.
- **[Marktdaten & Backtesting](/de/modules/market-data)**: OHLCV-Historie von mehreren Anbietern laden, jedes Instrument live oder auf Abruf mit Indikatoren charten, regelbasierte Strategien backtesten und Quant-Analysen auf Datensätzen, gespeicherten Backtests, Futures-Kurven und Optionsketten ausführen.
- **[Portfolios & Vermögen](/de/modules/portfolio)**: Live-Portfolioverfolgung, Vermögensverlauf, 13F-Bestände von Superinvestoren, Steuerschätzungen.
- **[News & Recherche](/de/modules/news-research)**: RSS-/API-News-Dashboards, Wirtschaftskalender, ein Suchkatalog mit 300.000 Instrumenten.
- **[Notizen & Organisation](/de/modules/productivity)**: Rich-Text-Editor mit Datenbanken, ToDos, Ziele, Kalender, Erinnerungen, Trading-Routinen und Mindset-Check-ins.
- **[KI-Agent](/de/modules/agent)**: integrierter Chat-Assistent (eigener Anbieter), der über MCP auf deine Daten zugreifen kann, mit Gedächtnis, Skills und externen MCP-Servern.

Siehe die [vollständige Modulliste](/de/modules/).

## Nächste Schritte

1. [OpenTraderWorld installieren](/de/guide/install): etwa 5 Minuten mit Docker.
2. [Erste Schritte gehen](/de/guide/first-steps): anmelden, Standardwerte wählen, Module installieren.
3. [Netzwerkzugriff konfigurieren](/de/config/network): wenn du von anderen Geräten darauf zugreifen willst.

Noch nicht bereit zur Installation? Probiere die [Live-Demo](https://demo.opentraderworld.com), eine geteilte Instanz
mit Beispieldaten, die alle 15 Minuten zurückgesetzt wird. [Was der Demo-Modus ist](/de/guide/demo) und was er blockiert.
