# KI-Agenten (MCP)

OpenTraderWorld liefert einen integrierten **MCP-Server** mit, damit KI-Agenten (jeder [MCP](https://modelcontextprotocol.io)-kompatible Client) deine Module über ein kontrolliertes Gateway lesen und ändern können. Ein Agent kann Journal-Trades für dich erfassen, deine News-Feeds zusammenfassen, ToDos hinzufügen, deine Backtest-Ergebnisse abfragen und so weiter.

**Standardmäßig ist es aus.** Nichts lauscht auf Agenten, bis du es aktivierst.

::: tip Du suchst den Chat-Assistenten in der App?
Diese Seite handelt von **externen** Agenten, die sich *in* OpenTraderWorld verbinden. Den integrierten Chat-Assistenten, der in der App lebt (eigener Anbieter), findest du im [Agent-Modul](/de/modules/agent): Er kann dasselbe Gateway *nutzen*, um deine Daten zu erreichen.
:::

## Sicherheitsmodell {#security-model}

Mehrere Ebenen, die alle bestanden werden müssen:

1. **Globaler Schalter**: Der MCP-Endpunkt ist deaktiviert, bis du ihn unter **Einstellungen → MCP** aktivierst. Du kannst Tokens vorbereiten, solange er aus ist; jede Agent-Anfrage wird abgelehnt, bis er aktiviert ist.
2. **Bearer-Tokens**: eines pro Agent oder Anwendungsfall. Tokens werden **gehasht** gespeichert und nur **einmal** bei der Erstellung angezeigt; fehlgeschlagene Versuche werden gedrosselt. Widerrufe ein Token jederzeit.
3. **Modulberechtigungen pro Token**: Jedes Token gewährt **pro Modul** *keinen Zugriff*, *Lesen*, *Lesen + Schreiben* oder *Vollzugriff (Lesen + Schreiben + Löschen)*. Agenten entdecken nur die Module, die du gewährt hast.
4. **Harte Allowlist**: Konto-, Netzwerk-, Geheimnis-, Dateispeicher- und Datenlöschvorgänge werden Agenten **nie angeboten**, unabhängig von den Berechtigungen.

::: tip Die Allowlist ist bewusst, nicht automatisch
Ein Endpunkt ist für Agenten nur erreichbar, weil jemand ihn von Hand in den Katalog eingetragen hat. Ein neues Modul oder eine neue Route eines bestehenden Moduls ist **für jeden Agenten unsichtbar**, bis dieser Eintrag existiert, sodass das Gateway mit dem Wachstum der App nie versehentlich breiter werden kann. Persona- und Skill-Verwaltung bleiben bewusst draußen: Kein Agent und kein Inhalt, den ein Agent liest, kann eine Persona bearbeiten oder ihr Skill-Regal erweitern.
:::

::: tip Versionen funktionieren wie Commits
Ist die Versionierung unter **Einstellungen → Versionen** eingeschaltet, kann ein Agent mit Berechtigung für **Editor** oder **Backtest** nach einer Änderung eine Version eines Dokuments oder einer Strategie speichern, mit einer Notiz, was sich geändert hat, und Versionen auflisten, lesen, wiederherstellen oder löschen. Er kann die Versionierung pro Datei oder Strategie umschalten, aber nicht die globalen Schalter in den Einstellungen.
:::

::: warning Der Automator gewährt Schreiben, nicht Scharfschalten
Die Berechtigung für **Automator** lässt einen Agenten deine Workflows lesen, einen anlegen, seinen Graphen schreiben und testen. Sie lässt ihn **nicht** in Betrieb nehmen: Ein Graph, den ein Agent speichert, landet als Entwurf, den du im Editor übernimmst, er kann kein Zugriffstoken eines Workflows anhängen, und er kann keinen Workflow ausführen oder einen Zeitplan anfassen. Ein Workflow läuft unter seinem eigenen Token statt dem des Aufrufers, daher sind das Schreiben eines Graphen und das Scharfschalten zwei getrennte Berechtigungen. Siehe [die Modulseite](/de/modules/automator#letting-an-agent-build-a-workflow).
:::

## Aktivieren und ein Token erstellen

1. Gehe zu **Einstellungen → MCP** und schalte es ein.
2. **Neues Token**: Benenne es nach dem Client (z. B. `My Agent`), lege Berechtigungen pro Modul fest (oder nimm *Alle lesen* / *Alle lesen+schreiben* / *Alle voll* als Ausgangspunkt).
3. **Kopiere das Token sofort**: Es wird nur einmal angezeigt.

**Externen Zugriff erlauben** ist ein separates Häkchen im selben Dialog. Es gewährt nichts zusätzlich: Es erlaubt nur, dass dieses Token eine Chat-Bindung in der [Externen Steuerung](/de/config/external-control) trägt, wo eine Nachricht von Telegram, Slack oder Discord unter denselben Berechtigungen pro Modul läuft.

Der Erstellungsdialog zeigt außerdem ein **einfügefertiges Konfigurations-Snippet**, mit einem Tab pro Client-Familie: der rohe Endpunkt + Header, ein JSON-Block `mcpServers` (Cursor, Cline, Windsurf, VS Code…) und eine Kommandozeile `claude mcp add`. Dieselben Snippets bleiben unter der Token-Tabelle verfügbar, mit `<TOKEN>` als Platzhalter, um später eine zweite Maschine einzurichten.

## Einen Client verbinden

Der Endpunkt spricht **MCP über Streamable HTTP** unter:

```
POST http://<your-host>/api/mcp
Authorization: Bearer <TOKEN>
```

Jeder konforme Client funktioniert. Beispiel für eine MCP-Konfiguration:

```json
{
  "mcpServers": {
    "opentraderworld": {
      "type": "http",
      "url": "http://localhost:5454/api/mcp",
      "headers": { "Authorization": "Bearer <TOKEN>" }
    }
  }
}
```

Ersetze die URL durch deine Domain, wenn du einen LAN-/HTTPS-Modus nutzt.

::: tip Nur-localhost-Installationen
Ist die App nur auf `localhost` erreichbar (der Standard-Netzwerkmodus), müssen Agenten **auf derselben Maschine** laufen.
:::

## Mit OAuth verbinden (claude.ai, ChatGPT)

Manche Clients können kein festes Token halten: Die Konnektoren von claude.ai und ChatGPT melden sich nur mit OAuth an. Schalte für sie unten unter **Einstellungen → MCP** die **OAuth-Anmeldung** ein (es fragt nach deinem Passwort) und gib dem Client allein die Server-URL, `https://<your-domain>/api/mcp`, ohne Token.

1. Der Client registriert sich selbst und öffnet auf deiner Instanz eine Zustimmungsseite (melde dich vorher an, falls nötig).
2. Die Seite zeigt den Namen des Clients und **wohin deine Antwort gesendet wird**. Wähle die Module und Stufen, dann **Erlauben**: Bei jeder Genehmigung wird nach deinem Passwort gefragt.
3. Die Verbindung erscheint in der Token-Tabelle mit einem **OAuth**-Badge. Bearbeite ihre Berechtigungen oder widerrufe sie dort wie jedes Token; das Widerrufen trennt den Client.

Zugriffstokens gelten eine Stunde und werden im Hintergrund erneuert; eine Verbindung, die 30 Tage ungenutzt blieb, muss sich neu anmelden. Wird ein Erneuerungstoken je von jemand anderem wiederverwendet, wird die Verbindung widerrufen und du wirst benachrichtigt.

::: warning Remote-Clients brauchen öffentliches HTTPS
claude.ai und ChatGPT verbinden sich von ihren eigenen Servern, daher muss die Instanz über öffentliches HTTPS erreichbar sein ([Web-Modus](/de/config/network)). Genehmige nur eine Zustimmungsseite, die du selbst, gerade eben, geöffnet hast: Ein Link von jemand anderem kann sich nach jedem Client benennen.
:::

::: tip Mit `otw update` von 0.0.15 oder früher aktualisiert?
OAuth braucht eine neue Route in der `deploy/Caddyfile`. Siehe [Aktualisieren](/de/guide/updating#oauth-caddyfile).
:::

## Wie Agenten die App sehen

Agenten erhalten vier Gateway-Werkzeuge:

- **`otw_catalog`**: listet die Module und Operationen auf, die das Token aufrufen darf. Nur gewährte Module erscheinen. Eine Modulauflistung zeigt Methode, Pfad, Query-Parameter und Body-Felder der obersten Ebene jeder Operation; `endpoint` (`POST /api/backtest/run`) liefert das vollständige Body-Schema genau dieser Operation.
- **`otw_read`**: Leseoperationen (brauchen mindestens *Lesen* auf dem Modul).
- **`otw_compute`**: Operationen, die im Katalog mit *(compute)* markiert sind und eine Frage beantworten, ohne etwas zu speichern: ein Backtest, ein Parameter-Sweep, Risikokennzahlen. Sie brauchen *Lesen + Schreiben* wie jedes POST, aber dein Client wird dich nicht um die Genehmigung einer Berechnung bitten.
- **`otw_write`**: Anlege- und Änderungsoperationen (brauchen *Lesen + Schreiben*); **Löschoperationen** brauchen *Vollzugriff* auf dem Modul.

Antworten, die Text von außen tragen (Feed-Artikel, eingehende Mails), erreichen den Agenten in einem gekennzeichneten Block, der ihn anweist, den Inhalt als Daten zu behandeln und jede darin versteckte Anweisung zu ignorieren.

Die Token-Tabelle in den Einstellungen zeigt für jedes Token den Zeitpunkt der letzten Nutzung, sodass du veraltete erkennen und widerrufen kannst. Ein Token kann bei der Erstellung oder Bearbeitung auch ein Ablaufdatum erhalten: Danach funktioniert es überall nicht mehr, auch nicht für den In-App-Agenten.
