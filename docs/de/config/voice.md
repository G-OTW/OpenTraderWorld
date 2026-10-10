# Sprachsteuerung

Steuere OpenTraderWorld mit deiner Stimme oder diktiere in jedes Textfeld. **Nur Push-to-talk**: Das Mikrofon öffnet sich, wenn du drückst, und schließt sich, wenn du loslässt. Dazwischen hört nichts zu.

**Standardmäßig ist es aus.** Schalte es unter **Einstellungen → Sprache & Tastenkürzel** ein.

## Das Mikrofon braucht HTTPS oder localhost {#https}

Browser geben einer Webseite das Mikrofon (und ihre eingebaute Spracherkennung) nur in einem **sicheren Kontext**: einer Seite, die über **HTTPS** ausgeliefert wird, oder die auf **localhost** geöffnet ist. Das ist eine Browser-Regel, keine von OpenTraderWorld, und keine Einstellung in der App kann sie aufheben.

| So öffnest du OTW | Sprache funktioniert? |
|---|---|
| Auf der Maschine, die es betreibt, `http://127.0.0.1:5454` oder `http://localhost:5454` | Ja |
| Modus [LAN + HTTPS](/de/config/network#lan-https) oder [Öffentlich](/de/config/network#public) | Ja |
| Modus [Lokales Netzwerk (LAN)](/de/config/network) über unverschlüsseltes HTTP, von einem anderen Gerät | **Nein**, der Browser blendet das Mikrofon aus |

Um Sprache vom Handy oder einem anderen Computer in deinem Netzwerk zu nutzen, stelle **Einstellungen → Netzwerk** auf **LAN + HTTPS**. Bei unverschlüsseltem HTTP zeigt die Sprach-Einstellungsseite eine Warnung, und die Mikrofon-Schaltfläche erklärt, warum sie nicht starten kann.

## Zwei Tastenkürzel, zwei Modi

| | Standard | Was mit dem Gesagten geschieht |
|---|---|---|
| **Befehl** | `Alt+V` (`⌥V` auf dem Mac) | Wird zu einem Aktionsplan, auch wenn ein Textfeld den Fokus hat. |
| **Diktat** | `Alt+Shift+V` (`⌥⇧V`) | Wird Wort für Wort in das Textfeld mit Fokus getippt. Nie als Befehl gelesen. |

Halte das Tastenkürzel gedrückt, während du sprichst, lass zum Beenden los, `Esc` bricht ab. Das **Mikrofon in der oberen Leiste** führt immer Befehle aus: zum Starten klicken, zum Stoppen erneut klicken oder wie ein Walkie-Talkie gedrückt halten.

Beide Tastenkürzel lassen sich unter **Einstellungen → Sprache & Tastenkürzel** ändern. Jedes braucht `Strg`, `Alt` oder `⌘` (oder eine Funktionstaste `F1` bis `F12`), damit es nie beim normalen Tippen stört, und die beiden müssen sich unterscheiden.

## Spracherkennung

Die Engine wandelt deine Aufnahme in Text um. Wähle eine unter **Einstellungen → Sprache & Tastenkürzel**.

| Engine | Einrichtung | Wohin das Audio geht |
|---|---|---|
| **Dieser Browser** | keine | Der Sprachdienst des Browser-Herstellers (Google bei Chrome, Microsoft bei Edge, Apple bei Safari). In Firefox nicht verfügbar. |
| **Selbst gehostetes Whisper** | der mitgelieferte Dienst, siehe [Beispiel: selbst gehostetes Whisper](#whisper-example) | Bleibt auf deiner Maschine |
| **whisper.cpp-Server** | dein eigener Server, sein `/inference`-Endpunkt | Bleibt auf deinem Server |
| **OpenAI / Groq** | ein API-Schlüssel (eingefügt oder aus dem [Tresor](/de/config/settings#vault) eingesteckt) | Wird an diesen Anbieter gesendet |
| **Anderer** | jeder Server, der die OpenAI-API `/audio/transcriptions` bereitstellt | Dieser Server |

Bei einer Server-Engine nimmt der Browser auf, wandelt das Audio in eine kleine WAV-Datei um, und OTW leitet sie an die Engine weiter. **Testen** sendet eine halbe Sekunde Stille, um URL, Schlüssel und Modell zu prüfen. Schlüssel sind im Ruhezustand verschlüsselt und werden nie an den Browser zurückgesendet.

### Beispiel: selbst gehostetes Whisper, von Grund auf {#whisper-example}

Der mitgelieferte Whisper-Dienst ist optional und wird von einem einfachen `up` nicht gestartet. Die Schritte 1 bis 3 erledigst du einmal: Das Modell bleibt über Neustarts hinweg im Volume `whisper-cache`.

**1. Den Dienst starten**, im Wurzelverzeichnis des Repositorys:

```bash
docker compose -f deploy/docker-compose.yml --env-file deploy/.env --env-file deploy/network.env --profile voice up -d whisper
docker logs -f opentraderworld-whisper-1
```

Warte auf `Uvicorn running on http://0.0.0.0:8000`, dann `Ctrl+C`. Der Port ist nur für OTW innerhalb von Docker erreichbar, nicht aus deinem Browser, und das ist gewollt.

**2. Das Modell herunterladen** (etwa 500 MB, ein paar Minuten):

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen(urllib.request.Request('http://localhost:8000/v1/models/Systran/faster-whisper-small',method='POST'),timeout=1800).read())"
```

**3. Prüfen, ob es installiert ist**: Die Antwort muss `Systran/faster-whisper-small` auflisten.

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen('http://localhost:8000/v1/models').read())"
```

**4. In OTW einstecken.** Öffne die App unter `http://localhost:5454` (oder über HTTPS, siehe [oben](#https)), dann **Einstellungen → Sprache & Tastenkürzel**:

1. **Erkennung hinzufügen**, Vorlage **Selbst gehostetes Whisper**: Es füllt `http://whisper:8000/v1` und `Systran/faster-whisper-small` ein.
2. **Speichern und testen**: Die Engine zeigt **Funktioniert · … ms**.
3. Wähle die Engine, dann stelle den Schalter oben auf **An**. Optional kannst du die **Sprache** festlegen.

**5. Ausprobieren**: Halte `⌥V` / `Alt+V`, sage *„open journal“*, lass los, drücke `Enter`. Für Diktat klicke in ein Textfeld und halte `⌥⇧V` / `Alt+Shift+V`.

Die erste Transkription nach einem Neustart ist langsamer, während das Modell in den Speicher geladen wird.

## Sprachbefehle

**Einstellungen → Sprachbefehle**: Ein Befehl besteht aus einem **Satz** (plus weiteren Formulierungen) und einer geordneten Liste von **Schritten**:

- **Eine Seite öffnen**: ein Modul, das Dashboard oder die Einstellungen.
- **Den Agenten fragen**: ein Prompt an den schwebenden Assistenten, der mit seinen eigenen Werkzeugen handelt.
- **Einen Workflow ausführen**: einen [Automator](/de/modules/automator)-Workflow starten.
- **Theme**, **Zahlen ausblenden**: dasselbe wie die Schaltflächen in der oberen Leiste.
- **Sprechen**: einen Satz vorlesen.

Ein Satz löst nur aus, wenn er **für sich allein** gesagt wird, nie als Wort innerhalb eines Satzes: Das Diktieren von „a blue turtle“ führt deinen Befehl *turtle* nicht aus.

### Eingebaut, ohne Einrichtung

- **„open &lt;page&gt;“** (*„ouvre &lt;page&gt;“*, *„öffne &lt;Seite&gt;“*, *„abre &lt;página&gt;“*, *„apri &lt;pagina&gt;“*, *„打开 &lt;页面&gt;“*) öffnet jedes installierte Modul, das Dashboard oder die Einstellungen. Ein Name, der auf mehrere Seiten passt, wird mit der Liste abgelehnt, nie geraten.
- Ist **Den Rest an den Agenten übergeben** an, geht alles, was kein Befehl trifft, als eine Anfrage an den Assistenten.

### Verketten

Sage mehrere Dinge auf einmal, verbunden durch *and* oder *then* (*et*, *puis* auf Französisch und so weiter, je nach Einstellung **Sprache**). Auch ein Komma im Transkript trennt:

> "turtle, then open settings and compare AAPL and MSFT"

führt deinen Befehl *turtle* aus, öffnet die Einstellungen und fragt dann den Agenten nach „compare AAPL and MSFT“. Übrig gebliebene, nebeneinanderliegende Teile bleiben zusammen, sodass der Agent die ganze Anfrage bekommt.

## Bestätigung

Jeder Plan wird **angezeigt, bevor er läuft**: was gehört wurde, jeder Schritt und woher er kam. `Enter` führt ihn aus, `Esc` bricht ab. Die Schritte laufen der Reihe nach, und der Plan stoppt beim ersten Fehler und nennt den Schritt.

Ein Befehl kann als **Ohne Bestätigung ausführen** markiert werden. Das ist **standardmäßig aus**, und es gilt nur, wenn dieser Satz allein gesagt wird: Mit etwas anderem verkettet wird der Plan trotzdem zuerst angezeigt. Agent-Schritte behalten die eigene Bestätigung des Assistenten für jeden Schreibvorgang.

## Gut zu wissen

- Der Assistent ist auf der Seite **Agent** ausgeblendet, daher kann ein Plan mit einem Agent-Schritt von dort nicht laufen.
- **Das Ergebnis vorlesen** nutzt die eigenen Stimmen des Browsers: keine Einrichtung, kein Audio verlässt den Browser.
- Die öffentliche Demo zeigt die Sprachseiten schreibgeschützt: Sie speichert keine Einstellungen und sendet kein Audio.
- **Dein eigener Reverse Proxy** vor OTW darf das Mikrofon nicht blockieren: Sein Header `Permissions-Policy` braucht `microphone=(self)`. Mit `microphone=()` verweigert der Browser sofort, ohne zu fragen.
