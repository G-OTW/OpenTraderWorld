# Externe Steuerung (Chat)

Steuere OpenTraderWorld über **Telegram, Slack oder Discord**. Eine Nachricht, die du an deinen Bot sendest, wird zu einem Lauf einer deiner [Agent-Personas](/de/modules/agent), im Chat beantwortet, mit genau dem Zugriff, den das gewählte Token erlaubt.

**Standardmäßig ist es aus**, und es bringt kein eigenes Berechtigungssystem mit: Die Obergrenze ist ein [MCP-Token](/de/config/ai-agents), dasselbe, das auch der In-App-Assistent nutzt.

## Nichts Neues lauscht auf deiner Maschine

Alle drei Transporte **wählen sich nach außen ein**: Telegram Long Polling, Slack Socket Mode, das Discord-Gateway. Es gibt keine öffentliche URL zu veröffentlichen, keinen Port zu öffnen und keine eingehende Route anzugreifen, daher funktioniert das unverändert auf der Standardinstallation nur mit localhost und hinter NAT.

Der Kanal, den du bereits für Benachrichtigungen nutzt, kann nur **senden** (eine Slack- oder Discord-Webhook-URL ist nur schreibbar). Zum Empfangen braucht es einen echten Bot, daher hält eine Bindung ihre eigenen Zugangsdaten:

| Plattform | Einzufügende Zugangsdaten | Auf der Plattform |
|---|---|---|
| **Telegram** | das BotFather-Bot-Token | sonst nichts |
| **Slack** | **beide** Tokens, `xapp-…` und `xoxb-…`, getrennt durch ein Leerzeichen oder einen Zeilenumbruch | Socket Mode an, Event `message.im`, Scope `chat:write` |
| **Discord** | das Bot-Token | der Intent **Direct Messages** (der privilegierte Message-Content-Intent wird für DMs nicht gebraucht) |

## Eine einrichten

1. **Einstellungen → Benachrichtigungen**: Lege den Kanal an, falls du keinen hast. Das ist der Antwortweg.
2. **Einstellungen → MCP**: Erstelle oder bearbeite ein Token, lege seine Stufen pro Modul fest und setze den Haken bei **Externen Zugriff erlauben**.
3. **Einstellungen → Externe Steuerung**: **Neue Bindung**, wähle den Kanal, die Persona und dieses Token, füge die Bot-Zugangsdaten ein (oder stecke sie aus dem [Tresor](/de/config/settings#vault) ein), lege optional Anbieter und Modell fest, mit denen neue Chats starten, und aktiviere die Bindung.
4. Schalte den Abschnittsschalter ein. Die Bindung zeigt innerhalb weniger Sekunden **Verbunden**.
5. **Koppeln**: Klicke auf *Koppeln*, sende dann den 6-stelligen Code an deinen Bot **von dem Konto aus, das steuern darf**. Er gilt einmal und so lange, wie *Kopplungscode gilt* angibt (standardmäßig eine Stunde, von 5 Minuten bis zu einem Tag).

Solange niemand gekoppelt ist, antwortet der Bot niemandem, auch dir nicht.

## Welches Modell antwortet

Jeder Chat trägt seinen eigenen Anbieter und sein eigenes Modell, genau wie eine Unterhaltung in der App. Die Bindung legt den **Standard fest, mit dem ein neuer Chat startet**: Eine Handy-Strecke verdient meist ein günstigeres, schnelleres Modell als dieselbe Persona im Browser. Lass es leer, und ein Chat übernimmt das der Persona.

Ändere den Standard im Bindungsformular. Ändere einen Chat aus dem Chat selbst:

- `/provider` listet die konfigurierten Anbieter, `/provider 2` oder `/provider openrouter` schaltet diesen Chat auf einen um (was das Modell auf den Standard dieses Anbieters zurücksetzt, da eine Modell-ID zu einem Anbieter gehört).
- `/model` listet die Modelle dieses Anbieters, `/model 3` wählt nach Position und `/model haiku` wählt per Text, wenn genau eine ID passt.

Die Wahl bleibt auf diesem Chat und ändert sonst nichts. `/new` verwirft die Unterhaltung, sodass die nächste wieder mit dem Standard der Bindung startet.

## Rechte

Das Token entscheidet alles, was eine Antwort berühren kann: *kein Zugriff*, *Lesen*, *Lesen + Schreiben*, *Vollzugriff* pro Modul, genau wie auf der [MCP-Seite](/de/config/ai-agents#security-model). Das Flag `external` erweitert nichts, es sagt nur, dass dieser Rahmen von außen erreichbar ist.

- **Ein Token pro Bindung, eine Bindung pro Kanal.** Das Widerrufen eines Tokens stoppt diese Bindung und sonst nichts, und die Spur zeigt weiterhin, auf welchem Weg ein Aufruf hereinkam.
- **Nutze ein eigenes Token** und beginne schreibgeschützt. Du kannst es später erweitern, ohne neu zu koppeln.
- Ein **abgelaufenes** Token oder eines, das das Flag verliert, stoppt die Bindung bei der nächsten Nachricht, nicht erst beim nächsten Neustart.

## Wer steuern darf

Ein Chat ist ein Ort, keine Identität, daher ist die Berechtigung an die **Sender-ID** der Plattform gebunden:

- Nur ein gekoppelter Sender bekommt eine Antwort. Alle anderen werden **ohne Antwort ignoriert**, und das ist Absicht: Eine Ablehnung verrät einem Fremden, dass der Bot echt ist.
- Gekoppelte Sender werden an der Bindung aufgeführt. Klicke auf einen, um ihn zu entfernen.
- **Nur Direktnachrichten.** Eine Gruppe würde mehreren Personen erlauben, in den Prompt eines Agenten zu schreiben, der eine Schreibberechtigung haben kann.

## Schreibvorgänge fragen immer nach

Jeder Schreibvorgang wird dir im Chat mit Methode, Pfad und Body vorgelegt und wartet auf ein Wort. Nur `yes`, `y`, `ok`, `okay`, `approve`, `oui` oder `go` genehmigen ihn; alles andere oder Schweigen lehnt ihn ab, und der Agent erfährt, dass er abgelehnt wurde.

Die Einstellung **Schreibvorgänge automatisch genehmigen** der Persona wird nicht übernommen. Sie wurde in einer authentifizierten Sitzung in der App gesetzt; sie folgt dir nicht aufs Handy.

## Was eine Nachricht durchläuft

Der Reihe nach, alles davon:

1. der globale Schalter unter **Einstellungen → Externe Steuerung**
2. die aktivierte Bindung
3. das Token trägt weiterhin `external` und ist nicht abgelaufen
4. ein Direktchat
5. der Sender steht auf der Allowlist
6. ein Rate-Limit von 12 Nachrichten pro Minute und Bindung

Dann unterliegt der Lauf selbst der Allowlist des MCP-Katalogs: Konto-, Netzwerk-, Geheimnis-, Dateispeicher- und Datenlösch-Routen sind unabhängig vom Token unerreichbar, ebenso die Verwaltung von Bindungen. Kein Agent kann eine Bindung anlegen, einen Kopplungscode erzeugen oder seine eigene Reichweite erweitern.

## Sicherheitshinweise

- **Das Bot-Token ist der Zugang.** Wer es hält, kann mit deiner Instanz auf der Stufe dieser Bindung sprechen, wobei die Sender-Allowlist weiter im Weg steht. Bewahre es im [Tresor](/de/config/settings#vault) auf und beginne schreibgeschützt.
- **Prompt Injection ist das eigentliche Risiko**, nicht der Transport. Inhalte, die der Agent liest (Mail, Feeds, eingehende Webhooks), können Anweisungen tragen. Eingedämmt wird es durch dasselbe wie in der App: die Allowlist des Katalogs, die Stufen des Tokens und die obige Schreibbestätigung.
- **Kopplungscodes** sind sechsstellig, einmalig und zeitlich begrenzt und existieren nur zwischen dem Klick auf *Koppeln* und der Einlösung. Verkürze das Zeitfenster im Abschnittskopf, wenn ein Code auf dem Bildschirm stehen bleibt.
- Bot-Zugangsdaten sind im Ruhezustand versiegelt und werden nie ausgegeben, auch nicht in Fehlermeldungen.
- Im [Demo-Modus](/de/guide/demo) komplett deaktiviert.

## Grenzen

- **Nur Text.** Keine Charts und keine Dateien; ein Chart lebt weiterhin in der App.
- **Kein Streaming Token für Token.** Chat-Plattformen bieten nur Nachrichtenbearbeitungen an und begrenzen deren Rate, daher kommt die Antwort in Blöcken von etwa anderthalb Sekunden und wird an der Obergrenze der Plattform geteilt (4096, 3000 und 2000 Zeichen).
- Befehle: `/new` startet eine frische Unterhaltung für diesen Chat, `/provider` und `/model` listen auf und schalten um, womit dieser Chat antwortet, `/whoami` zeigt die ID, als die du gekoppelt bist, `/help`.
- Ein Neustart verwirft eine ausstehende Schreibbestätigung. Nichts läuft unbestätigt, du wirst einfach erneut gefragt.
