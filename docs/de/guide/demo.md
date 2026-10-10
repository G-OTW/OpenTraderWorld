# Demo-Modus

Es gibt eine öffentliche Sandbox unter **[demo.opentraderworld.com](https://demo.opentraderworld.com)**: die echte App, mit Beispieldaten gefüllt, von allen gemeinsam genutzt und alle **15 Minuten** zurückgesetzt. Nichts zu installieren, keine Anmeldung nötig; du landest angemeldet als `demo`.

Diese Seite erklärt, was dieser Modus ist, damit du weißt, was du vor dir hast, und damit du selbst einen betreiben kannst, wenn du jemandem die App zeigen willst.

## Was es ist

Der Demo-Modus ist eine Haltung, die das Backend einnimmt, wenn es mit `OTW_DEMO=1` startet. **Er wird nie implizit aktiviert**: Eine normale Installation bleibt von allem Folgenden unberührt.

- **Die Datenbank wird zur Viertelstunde zurückgesetzt.** Das Seed wird aus einer Vorlage wiederhergestellt, alles, was du änderst, ist also um :00, :15, :30 oder :45 weg. Das Banner in der App zählt bis zum nächsten Zeitpunkt herunter.
- **Du wirst automatisch angemeldet** als Konto `demo`. Es gibt kein Passwort zu erraten und kein Konto anzulegen.
- **Alle teilen sich eine Datenbank.** Trades, Notizen und Unterhaltungen anderer Besucher sind sichtbar, und deine sind es für sie. Tippe nichts ein, was du nicht veröffentlichen würdest.

## Was blockiert ist

Das Gate ist **standardmäßig verweigernd**: Eine Anfrage muss zu einer ausdrücklichen Allowlist passen, sonst wird sie mit `demo_disabled` abgelehnt. Eine Route, an die niemand gedacht hat, ist geschlossen statt offen, dieselbe Regel, der auch der [MCP-Katalog](/de/config/ai-agents) folgt.

Grob:

| Blockiert | Nur lesend | Vollständig |
|---|---|---|
| Einrichtung, Abmelden, Konto & Passwort, Netzwerk, Sicherung, Update, Datenlöschung, Modul installieren/trennen, **der Tresor**, **der Automator**, eingehende Webhooks, der MCP-Endpunkt selbst, Benachrichtigungskanäle, FinanceDatabase-Installation, Anbieter-Downloads | Daten-Konnektoren, Feeds, Dateien, Managers' Portfolios, MCP-Einstellungen & Tokens, API-Rate, gespeicherte Datensätze, Agent-Anbieter, Erinnerungen und Skills | Journal, Backtest, Quant, Portfolios, Kalender, ToDos, Ziele, Editor & Datenbanken, Prompts, Ressourcen, Abos, Taxcalc, Zeiterfassung, Dashboard, Suche, Agent-Chat |

Du kannst also einen Trade erfassen, einen Backtest ausführen und mit dem Assistenten sprechen; du kannst nicht den Netzwerkmodus ändern, ein Token erzeugen, die Datenbank herunterladen oder die Box in deinem Namen bei einem kostenpflichtigen Anbieter abrufen lassen.

Zwei davon verdienen ein Wort, da das Modul sichtbar ist, aber nichts tut:

- **Der Tresor** ist komplett geschlossen. Er ist der eine Speicher, dessen ganzer Zweck das Halten von Zugangsdaten ist, und diese Datenbank ist geteilt und öffentlich.
- **Der Automator** ist durch die Regel „standardmäßig verweigern“ geschlossen, nicht durch eine eigene Zeile: Ein Workflow erreicht alles, was sein Token erlaubt, und kann jede URL aufrufen, genau das, was eine öffentliche Sandbox nicht bieten darf. Das Modul zu öffnen zeigt die Oberfläche; jede Anfrage dahinter antwortet mit `demo_disabled`.

Chat-Nachrichten sind außerdem auf 2000 Zeichen begrenzt.

## Kontingente pro Besucher

Die teuren Endpunkte kosten echtes Geld, daher hat jeder **zwei** Budgets: einen Anteil pro Besucher und obendrauf eine globale Obergrenze. Nur pro IP würde die Ausgaben nicht deckeln; nur global könnte ein einzelner skriptender Besucher alle anderen aussperren.

| | Pro Besucher | In der ganzen Demo | Zeitfenster |
|---|---|---|---|
| **Agent-Läufe** | 3 | 8 | 10 Minuten |
| **Agent-Läufe** | 10 | 40 | 24 Stunden |
| **Backtests & Sweeps** | 10 | 30 | 10 Minuten |

Der Assistent läuft mit einem **gemeinsamen Schlüssel, der auf ein kostenloses Modell festgelegt** ist (beim Start aufgelöst); Langzeitgedächtnis und externe MCP-Server sind aus.

## Selbst betreiben

```bash
OTW_DEMO=1        # in the core service's environment
otw-core --seed-demo   # once, against a scratch database
```

Das Seed ist öffentlich: Es liegt im Repo, enthält keine Geheimnisse und bricht ab, wenn bereits ein Benutzer `demo` existiert. Das Zurücksetzen ist eine `CREATE DATABASE … TEMPLATE`-Wiederherstellung, die vom Host aus gesteuert wird, nicht von der App.

::: warning Richte den Demo-Modus nicht auf deine Daten
Das Seed schreibt in die Datenbank, die `DATABASE_URL` nennt, und das Zurücksetzen stellt darüber wieder her. Nutze eine Wegwerf-Datenbank.
:::
