# Automator

*Workflows, die deine eigene App steuern.* Ein Workflow ist eine Liste von Schritten: einen Endpunkt deiner OpenTraderWorld-API aufrufen, eine beliebige URL außerhalb davon aufrufen, deinem KI-Anbieter eine Frage stellen, die Antwort umformen, dich selbst benachrichtigen. Führe ihn von Hand aus oder nach Zeitplan.

Typische Anwendungen: ein Montags-Briefing vor Marktöffnung, das die Wirtschaftsereignisse der Woche und deine Watchlists liest und eine einzige Telegram-Nachricht pusht; ein nächtlicher Export des Journals an einen externen Dienst; ein Preis-Webhook, der in eine Benachrichtigung aufgefächert wird; eine wöchentliche Zusammenfassung, die der Assistent aus deinen eigenen Zahlen schreibt.

Das Modul lebt unter **/automator** mit vier Bereichen: **Workflows**, **Zeitpläne**, **Agenda** (ein Kalender der kommenden Läufe), **Läufe**.

## Das Raster

Der Editor ist ein Raster, keine Zeichenfläche: Es sind keine Leitungen zu ziehen.

- **Schritte laufen von oben nach unten, Aufgaben innerhalb eines Schritts von links nach rechts.** Alles läuft nacheinander. Ein Schritt gruppiert die Aufgaben, die zum selben Moment des Workflows gehören, er startet sie nicht gemeinsam.
- Ziehe einen Block aus der Palette auf eine **Lücke**: Die Lücke zwischen zwei Schritten öffnet einen neuen Schritt, die Lücke zwischen zwei Aufgaben legt ihn in diesen Schritt. Jedes gültige Ablageziel wird hervorgehoben, bevor du loslässt.
- Eine Aufgabe kann nur lesen, was **vor** ihr lief. Verschiebe eine Karte, und ihre Referenzen werden beim nächsten Speichern erneut geprüft.
- **Autosave** 1,2 s nach jeder Änderung, **Strg/Cmd+Z** zum Rückgängigmachen. Ein Block, der noch ausgefüllt wird, bleibt als **Entwurf** erhalten: Er läuft nie, und kein Zeitplan greift ihn auf, bis er validiert.

## Blöcke

| Block | Was er tut |
|---|---|
| **App-Aufruf** | Ein Endpunkt deiner eigenen API, In-Process ausgeführt. Die Palette listet die Endpunkte, die ein Workflow erreichen darf; klicke einen an, und der Block landet bereits darauf ausgerichtet, mit der erwarteten Body-Form zur Hand. |
| **Web-Aufruf** | Eine beliebige URL außerhalb der App: Methode, Query, Header, JSON-/Text-/Formular-Body, Antwort gelesen als JSON, Text, CSV oder binär, mit einem Limit für die Antwortgröße. |
| **KI-Schritt** | Eine Modellrunde, ohne Unterhaltung drumherum. Wähle eine Persona und einen gespeicherten Prompt oder schreibe die Anweisungen; verlange ein **JSON-Objekt**, wenn der nächste Block Felder statt Prosa lesen muss. Er ruft deinen Anbieter auf und kostet bei jedem Lauf Tokens. |
| **Transformieren** | Formt um, was davor kam: einen Wert mit `pick` wählen, ein Objekt mit `set` setzen, einen String mit `format` bilden, ein CSV mit `csv_parse` parsen, eine Liste mit `join` zu einer Zeile verbinden. |
| **Benachrichtigen** | Eine In-App-Benachrichtigung und deine [Benachrichtigungskanäle](/de/config/settings#notifications) (E-Mail, Telegram, Slack, Discord). Ist nichts gewählt, gilt jeder aktivierte Kanal, der dem Automator gewährt ist. |
| **Warten** | Den Lauf pausieren. **Stopp** antwortet auch während des Wartens. |

## Daten zwischen Blöcken weitergeben

Jeder Block hat eine **ID**, oben in seinem Editor angezeigt. Ein späterer Block liest sein Ergebnis mit einem Ausdruck:

```
{{steps.http1.output}}                     the whole answer
{{steps.http1.output.items.0.name}}        one field of it
{{steps.http1.status}}                     ok | simulated | failed | skipped
{{steps.http1.error}}                      the failure message, empty on success
{{run.started_at}} {{run.trigger}} {{workflow.name}} {{input.key}}
```

Filter werden nach einem Pipe verkettet: `json`, `upper`, `lower`, `trim`, `round:2`, `date:"DD/MM/YYYY"`, `default:"n/a"`. Nur `default` rettet einen Wert, der nicht da ist.

Das ist **keine Sprache**: keine Arithmetik, kein Code. Jede Referenz wird **beim Speichern des Graphen** gegen die Blöcke geprüft, die ihr tatsächlich vorausgehen, ein defekter Verweis wird also im Editor abgelehnt statt um drei Uhr morgens.

## Geheimnisse

Ein Passwort oder ein API-Schlüssel gehört in den [Tresor](/de/config/settings#vault), nie in ein Feld getippt. Verweise darauf mit <code v-pre>{{vault.myvault.mykey}}</code> in einem Header, einem Query-Wert oder einem Request-Body, den einzigen Stellen, an denen es akzeptiert wird (nie in einer URL), und der aufgelöste Wert wird aus dem Laufverlauf getilgt. Ein Query-Wert erreicht trotzdem die Logs der aufgerufenen Seite, bevorzuge also einen Header.

## Berechtigungen

- **Ein App-Aufruf braucht ein Zugriffstoken.** Wähle eines in den **Einstellungen** des Workflows; Tokens werden unter [Einstellungen → MCP](/de/config/ai-agents) erstellt. Kein Token heißt gar kein interner Aufruf, und ein Workflow kann nur erreichen, was sein Token gewährt, innerhalb derselben Allowlist, die das MCP-Gateway nutzt.
- **Ein Web-Aufruf verweigert dein eigenes Netzwerk.** Loopback-, private, CGNAT- und Link-Local-Adressen werden abgelehnt, sofern der Block interne Ziele nicht ausdrücklich erlaubt. Der Host wird zuerst aufgelöst und die Verbindung auf die geprüfte Adresse festgelegt, und jeder Redirect-Sprung wird erneut geprüft.
- **Benachrichtigen braucht eine Freigabe.** Ein Kanal bietet sich hier erst an, nachdem dem Automator unter Einstellungen → Benachrichtigungen die Freigabe erteilt wurde.

## Ausprobieren, dann ausführen

- **Testlauf** führt die Lesevorgänge aus und meldet, was ein Schreib- oder Sendevorgang *getan hätte*, sodass nichts die App verlässt. Ein Block, der einen Wert liest, den nur ein simulierter Schreibvorgang hätte erzeugen können, wird selbst als **simuliert** gemeldet, statt einen Test scheitern zu lassen, den ein echter Lauf bestehen würde.
- **Diesen Block testen** führt einen Block allein aus, nach denselben Regeln.
- **Jetzt ausführen** tut es wirklich. **Stopp** wird zwischen Blöcken und während eines Wartens geprüft; ein Lauf jenseits des **Laufzeitlimits** des Workflows wird als Timeout beendet.

## Läufe

Jeder Lauf behält **eine Zeile pro Block**: was gesendet wurde, was zurückkam, den Status und das Timing, sodass ein um 3 Uhr morgens gescheiterter Workflow den Block nennt und die Nutzlast zeigt. Eine Ausgabe über 256 KB wird ausgelagert und auf Anforderung geholt. Der Verlauf wird auf die letzten 50 Läufe pro Workflow gekürzt (10 bei Testläufen).

## Zeitpläne

Alle N Minuten, täglich, an einigen Wochentagen, monatlich oder einmalig zu einem bestimmten Zeitpunkt, jeweils mit eigener **IANA-Zeitzone**, sodass eine in Europe/Paris gesetzte Regel Paris folgt und nicht dem Server.

- **Keine Überlappung**: Läuft noch ein Lauf, wenn das nächste Vorkommen fällig wird, wird dieses Vorkommen **verworfen**, nicht eingereiht.
- **Nachholen** (optional): War die App zur geplanten Zeit aus, läuft sie einmal beim Start.
- Die Sommerzeit wird aufgelöst, nicht ignoriert: Eine ausgefallene lokale Stunde läuft am Ende der Lücke, eine doppelte läuft einmal. Eine monatliche Regel, die hinter das Ende eines kurzen Monats gesetzt ist, läuft an dessen letztem Tag.
- Ein Zeitplan kann an eine **Version** des Workflows **gepinnt** werden. Das Speichern eines neuen Graphen fragt, ob die gepinnten Zeitpläne ihm folgen sollen; Autosave richtet nie von selbst einen um.
- **Pausieren / Fortsetzen** über die Workflow-Karte oder die Zeitplan-Liste. Ein deaktivierter Workflow wird nie von einem Zeitplan gestartet, das Ausführen von Hand funktioniert aber weiterhin.

::: warning Ein geplanter Schreibvorgang schreibt
Ein App-Aufruf, der auf einen Endpunkt zeigt, der deine Daten ändert, tut das bei jedem Lauf unbeaufsichtigt. Der Editor markiert diese Endpunkte; teste den Workflow, bevor du ihn planst.
:::

## Einen Agenten einen Workflow bauen lassen {#letting-an-agent-build-a-workflow}

Einen Graphen zusammenzustellen ist das Schwierigste, was dieses Modul von dir verlangt, und genau die
Art von Arbeit, in der ein [KI-Agent](/de/config/ai-agents) gut ist. Ein Agent mit einem Token, das die
Berechtigung **Automator** hält, kann also deine Workflows lesen, einen anlegen, seinen Graphen schreiben und testen.

In Betrieb nehmen kann er ihn nicht. Die Trennung ist gewollt:

- Ein Graph, den ein Agent speichert, landet als **Entwurf**, nie als der Graph, der läuft. Öffne den
  Workflow, lies, was er geschrieben hat, und Speichern übernimmt ihn. Bis dahin ändert sich nichts: Ein
  Workflow, der bereits nach Zeitplan läuft, führt weiter die Version aus, die du gespeichert hast.
- Ein Agent kann das **Zugriffstoken** eines Workflows nicht anhängen. Dieser Rahmen verwandelt einen Graphen
  in Berechtigungen, du erteilst ihn also von Hand, nachdem du den Graphen gelesen hast, der laufen wird.
- Ein Agent kann einen Workflow nicht **ausführen**, keine Revision wiederherstellen, keinen löschen und keinen Zeitplan anfassen.
- Seine **Testläufe** sind versiegelt: Benachrichtigungen und externe Aufrufe sind erzwungen aus, und ohne
  angehängten Rahmen schlägt ein App-Aufruf geschlossen fehl. Ein Test prüft, dass der Graph läuft, seine
  Ausdrücke sich auflösen und seine Transformationen tun, was sie versprechen, ohne irgendetwas zu erreichen. Ein
  Workflow, der bereits ein Token trägt, wird abgelehnt: Den testest du selbst.

Der Grund für die Grenze ist, dass ein Workflow unter **seinem eigenen** Token läuft, nicht unter dem des Aufrufers.
Ein Agent, der einen Graphen sowohl schreiben als auch starten könnte, erbte alles, was dieses Token gewährt,
was auch immer seine eigenen Berechtigungen sagten. Schreiben und Scharfschalten sind zwei Berechtigungen, und nur eine davon
kannst du delegieren.

Der Automator ist im [Demo-Modus](/de/guide/demo) deaktiviert.
