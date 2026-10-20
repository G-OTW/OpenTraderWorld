# Agent

Ein integrierter **KI-Chat-Assistent**: ein Chat-Bereich in der App, der auch auf deine OpenTraderWorld-Daten zugreifen kann. Öffne ihn über das Modul **Agent**, das Funken-Symbol in der oberen Leiste (neben der Suche) oder die [schwebende Schaltfläche](#the-floating-assistant) in der Ecke jeder Seite.

Der Assistent ist **Bring-your-own-Provider**: Nichts ist aktiviert oder herstellerseitig voreingestellt, bis du einen eigenen Anbieter und Schlüssel hinzufügst.

::: tip Zwei verschiedene „KI-Agenten“
Diese Seite handelt vom **Chat-Assistenten, der in der App lebt** und mit einem Anbieter spricht, den *du* konfigurierst. Das ist nicht dasselbe wie die Seite [KI-Agenten (MCP)](/de/config/ai-agents), die von *externen* Agenten handelt, die sich über den ausgehenden MCP-Server **in** OpenTraderWorld verbinden. Der Chat-Assistent kann dasselbe Gateway *nutzen*, um deine Daten zu erreichen: siehe [Werkzeuge](#tools-over-your-data) unten.
:::

## Einen Anbieter hinzufügen

Füge in den **Einstellungen** (das Zahnrad in der Chat-Seitenleiste) → **Allgemein** einen oder mehrere Anbieter hinzu. Zwei Wire-Formate werden unterstützt:

- **Anthropic**: die Claude Messages API.
- **OpenAI-kompatibel**, also jeder Endpunkt, der das OpenAI-Chat-Format spricht: OpenRouter, OpenAI, DeepSeek, Moonshot, Groq, Mistral, der Kompatibilitäts-Endpunkt von Gemini, ein lokaler Proxy und so weiter.

Jeder Anbieter hat seine eigene **Basis-URL** (nur OpenAI-kompatibel), seinen **API-Schlüssel** und sein **Standardmodell**. Der Schlüssel ist **nur schreibbar**: Er wird im Ruhezustand mit dem Hauptschlüssel der App verschlüsselt und nach dem Speichern nie wieder angezeigt. Lass das Schlüsselfeld beim Bearbeiten leer, um den aktuellen zu behalten.

Ein Anbieter kann deaktiviert werden, ohne ihn zu löschen. Der Assistent ist erst „bereit“, wenn er einen aktivierten Anbieter mit Schlüssel und Modell hat.

## Den Assistenten konfigurieren

Im selben Einstellungsbereich legst du den **System-Prompt**, den aktiven **Anbieter / das Modell**, **Max. Tokens** und **Temperatur** fest. Ein Feld **Erweiterte Parameter (JSON)** reicht beliebige zusätzliche Request-Felder unverändert an den Anbieter weiter. Setze einen Wert auf `null`, um einen Schlüssel zu *entfernen*, den die App sonst senden würde (z. B. `max_completion_tokens` für neuere OpenAI-Modelle oder das Weglassen von `stream_options`).

## Chat

- Antworten werden **live gestreamt** und als Markdown dargestellt. Modelle, die ihr Reasoning offenlegen, erhalten eine optionale Einklappfläche **Thinking**.
- Unterhaltungen werden in der **Seitenleiste** gespeichert: neu, auswählen, umbenennen, löschen und **Markdown-Export** per Klick.
- **In den Editor übertragen** macht aus einer Unterhaltung eine Editor-Seite: Wähle einen Ordner (oder lege einen an) und einen Dateinamen, dann **Speichern**, um beim Agent zu bleiben, oder **Speichern und öffnen**. Jede Nachricht behält Datum und Autor (Du, oder die Persona und das Modell), Reasoning steht in einem Zitat über der Antwort, und **Details einschließen** fügt Tool-Aufrufe und Token-Zahlen hinzu.
- Du kannst einen Lauf mitten im Stream **stoppen**.
- Die Chat-Kopfzeile zeigt einen laufenden **Token-Zähler** (Eingabe + Ausgabe) für die Unterhaltung, damit du siehst, was ein Thread kostet.
- Ein Schalter **Breiter Modus** hebt die Lesebreitenbegrenzung auf. Der Thread ist für Prosa dimensioniert, was die falsche Form für die Tabellen ist, die der Assistent erzeugt: Ein Trial-Ledger oder eine Statistik-Aufschlüsselung braucht den Platz. Die Wahl bleibt pro Browser erhalten.
- Anbieterfehler erscheinen als einfacher Satz in einem schließbaren Banner: ein abgelehnter Schlüssel, ein Rate-Limit (mit dem Retry-After des Anbieters, wenn er eines sendet), ein falsches Modell oder eine falsche Basis-URL, eine Ablehnung durch den Inhaltsfilter oder eine am Token-Limit abgeschnittene Antwort.

### Chat-Limits

Ein Chat kann bei **Ausgabe-Tokens**, bei **Dollar** oder bei beidem begrenzt werden. Lege die Standardwerte unter **Einstellungen → Allgemein → Limits für neue Chats** fest; jeder neue Chat startet damit, und spätere Änderungen lassen bestehende Chats unberührt. Lass ein Feld leer für kein Limit.

Die schmale Leiste neben der Schaltfläche Prompt Store im Nachrichtenfeld füllt sich von unten nach oben, während der Chat ausgibt. Fahre darüber, um die Zahlen zu sehen, klicke darauf, um die Limits dieses Chats zu ändern. Wird ein Limit erreicht, stoppt der Lauf vor seinem nächsten kostenpflichtigen Schritt, und der Chat nimmt keine neue Nachricht an, bis du das Limit erhöhst oder entfernst, was du über dieselbe Leiste tun kannst. Senden am Limit zeigt einen Hinweis mit zwei Abkürzungen: **Limit ändern** öffnet diesen Editor, **Neuer Chat** öffnet einen frischen Chat mit derselben Persona und behält, was du getippt hast.

Das Dollar-Limit braucht einen Anbieter, der den Preis mit jeder Antwort zurückgibt (OpenRouter tut das, Anthropic nicht). Ohne ihn erscheint die Ausgabe als „kein Preis geliefert“, und nur das Token-Limit gilt.

### Anbieter oder Modell pro Chat wechseln

Die Chat-Kopfzeile zeigt den aktiven **Anbieter · Modell**. Sie zu öffnen zeigt dir eine **Vollbild-Auswahl**: auf einer Seite die Anbieter, auf der anderen die **Live-Modellliste** des gewählten Anbieters, durchsuchbar und serverseitig abgefragt, sodass dein Schlüssel nie den Browser erreicht. Freitext funktioniert weiterhin für Proxys, die keine Liste bereitstellen. Nichts ändert sich, bis du mit **Dieses Modell verwenden** bestätigst, das Durchstöbern der Liste kostet also nichts, und eine einzelne Schaltfläche setzt die Unterhaltung auf den geerbten Standard zurück.

Die Wahl gehört **zu dieser Unterhaltung**, nicht zum Assistenten: Ein günstiges schnelles Modell kann in einem Tab dein Journal prüfen, während das stärkste Reasoning-Modell in einem anderen über einen Backtest streitet. Eine Unterhaltung ohne eigene Wahl erbt die der Persona, dann das, was du unter **Einstellungen → Allgemein** festgelegt hast. Ein Punkt in der Auswahl markiert die, die mit etwas Eigenem laufen, und ein Klick setzt sie auf die geerbte Einstellung zurück. Das Wechseln des Anbieters löscht die Modell-ID mit, da ein Modellname nur dem Hersteller etwas sagt, von dem er stammt.

### Einen gespeicherten Prompt senden

Das Eingabefeld kann aus deinem [Prompt Store](/de/modules/productivity#prompt-store) schöpfen, statt einen Prompt neu zu tippen, den du aufbewahrst. Die Auswahl listet deine Prompts mit einer Suche über Name, Tags und Inhalt, zeigt den gewählten vollständig in der Vorschau und fügt ihn bei **Prompt einfügen** (oder einem Doppelklick auf die Zeile) ins Eingabefeld ein, wo du ihn vor dem Senden noch bearbeiten kannst.

## Der schwebende Assistent {#the-floating-assistant}

Eine Schaltfläche in der **Ecke unten rechts auf jeder Seite** öffnet einen kompakten Chat über deinen bestehenden Unterhaltungen, ohne zu verlassen, was du gerade tust. Es ist derselbe Assistent, kein paralleler: dieselben Unterhaltungen, Personas, Anbieter und Werkzeuge, sodass ein in der Ecke begonnener Thread danach auf der Seite Agent ist und umgekehrt. Persona, Modell und Werkzeuge stehen in einer Zeile unter dem Titel, und die Auswahl für Modell und Prompt öffnet sich als Ansicht über dem Thread statt als Dialog.

Er weiß auch, **auf welcher Seite du bist**. Jede Nachricht trägt das aktuelle Modul, und wenn das Token der Unterhaltung dieses Modul gewährt, wird dessen Endpunktliste gleich zu Beginn in den Prompt geladen, sodass eine Anfrage aus Historical Data ihre erste Tool-Runde nicht damit verbringt herauszufinden, wo sie suchen soll. Alles andere bleibt einen Abruf entfernt, und eine Seite, mit der der Assistent nichts zu tun hat (Einstellungen, das Dashboard), sendet gar nichts.

## Gedächtnis & Skills

Zwei Tabs in den Einstellungen lassen den Assistenten Wissen über Unterhaltungen hinweg mitnehmen:

- **Gedächtnis**: kleine, dauerhafte Fakten (eine Vorliebe, ein stabiles Detail), die über Chats hinweg bestehen. Nur der **Index** (Slug + einzeilige Beschreibung) reist im Prompt mit; der volle Inhalt wird bei Bedarf geholt. Du durchsuchst, bearbeitest und löschst jede Erinnerung selbst, nichts ist verborgen. Jede Erinnerung vermerkt, **welche Persona sie geschrieben hat**, sowohl im Manager als auch im Index, den der Assistent liest: Das Gedächtnis ist ein gemeinsamer Speicher, eine vom Day Trader geschriebene Einschränkung würde für den Analysten sonst wie seine eigene klingen. Der Assistent kann Erinnerungen auch selbst ausdünnen, wenn der Speicher voll wird, und kann eine von dir von Hand geschriebene nicht stillschweigend überschreiben.
- **Skills**: wiederverwendbare Markdown-Anweisungssätze, die du definierst. **Name + Beschreibung** eines Skills sind immer im Kontext; der Assistent lädt den vollen Inhalt bei Bedarf, wenn eine Aufgabe ihn verlangt. Aktiviere/deaktiviere jeden Skill einzeln.

Lange Unterhaltungen erhalten außerdem eine **rollierende Zusammenfassung**: Sobald ein Chat groß wird, werden ältere Runden zu einer laufenden Zusammenfassung verdichtet, damit der Thread günstig bleibt, und nur die jüngsten Nachrichten bleiben wörtlich erhalten.

## Personas

Eine **Persona** ist eine rollenförmige Version des Assistenten: ein System-Prompt mit einer Haltung und einer ausdrücklichen Ablehnungsgrenze, plus ein kuratiertes **Skill-Regal**. Fünf werden eingebaut mitgeliefert (**Quant**, **Portfolio Manager**, **Day Trader**, **Researcher**, **Financial Analyst**), und du wählst eine beim Öffnen einer Unterhaltung über die Persona-Auswahl in der Chat-Kopfzeile.

Die Idee ist Enge. Ein generischer Assistent mit zweihundert Endpunkten ist bei jeder einzelnen Aufgabe schlechter als einer, der wenige davon gründlich kennt und den Rest ablehnt. Der Quant meldet keinen Backtest ohne die Trial-Anzahl und eine Out-of-Sample-Zahl; der Day Trader nennt keinen Einstieg; der Analyst meldet die Fundamentaldaten, die er *nicht* beschaffen konnte, statt sie aufzufüllen.

### Was eine Persona nicht ist

**Eine Persona ist kein Berechtigungssatz.** Was der Assistent erreichen kann, ist das MCP-Token der Unterhaltung: modulbezogen, von dir gesetzt, identisch, welche Persona auch spricht. Das Wechseln der Persona verengt die *Haltung und das Regal*, nie den Datenzugriff. Um zu ändern, was er berühren kann, ändere das Token.

### Mitten in der Unterhaltung wechseln

Du kannst die Persona mitten im Thread wechseln. Es wirkt **ab der nächsten Nachricht**, und eine Markierung im Transkript hält die Übergabe fest: Die Runden darüber wurden von der vorherigen Persona erzeugt und bleiben ihr zugeordnet.

### Eigene bearbeiten

**Einstellungen → Personas** listet jede Persona mit dem Regal, das sie tatsächlich bekommt. Von dort kannst du:

- eine von Grund auf **erstellen** oder eine mitgelieferte **duplizieren** und die Kopie umschreiben;
- den **Prompt**, das **Regal** bearbeiten und ob sie **Schreibvorgänge automatisch genehmigt**;
- eine eingebaute auf den Auslieferungszustand **zurücksetzen** (deine Änderungen daran gehen verloren, sonst wird nichts berührt);
- eine selbst erstellte **löschen**. Ihre Unterhaltungen bleiben **erhalten**: Sie wechseln zum Standard-Assistenten, und jedes Transkript erhält einen Vermerk dazu. Eingebaute können nicht gelöscht werden: Die App erstellt sie beim nächsten Neustart neu, ein Löschen würde also nur scheinbar funktionieren.

Skills werden hier **nicht** erstellt. Es gibt einen Katalog, verwaltet im Tab Skills, und Personas wählen daraus. Das heißt, das Bearbeiten eines Skill-Inhalts ändert ihn für jede Persona, die ihn hält, und die Skill-Liste zeigt wie viele, sodass die Änderung nie blind ist.

### Export und Import

Jede Persona exportiert als **JSON-Datei mit eingebetteten Skill-Inhalten**, sodass eine Datei sie auf einer anderen Maschine reproduziert. Du kannst auch einen einzelnen Skill oder das ganze Regal auf einmal exportieren. Der Import akzeptiert beide Formen; ein vorhandener Name wird übersprungen statt überschrieben. Es gibt kein Prüf-Gate: Das ist deine Maschine, und was du in deinen eigenen Assistenten lädst, ist deine Entscheidung.

Persona- und Skill-Verwaltung fehlen bewusst im **MCP-Katalog**: Kein Agent und kein Inhalt, den ein Agent liest, kann eine Persona bearbeiten oder ein Regal erweitern.

## Schreibbestätigung

Will der Assistent deine Daten ändern, **pausiert** der Lauf und zeigt dir den genauen Aufruf (Methode, Pfad und Body) mit Genehmigen und Ablehnen. Nichts wird geschrieben, bevor du antwortest, und eine Ablehnung wird dem Modell als Verweigerung gemeldet statt als Fehler, den es umgehen soll. Gehst du weg, läuft das Warten in ein Timeout, und der Schreibvorgang findet nicht statt.

Eine Persona kann auf **Schreibvorgänge automatisch genehmigen** gestellt werden, was die Abfrage bei gewöhnlichen Änderungen überspringt. **Löschungen fragen immer nach**, was diese Einstellung auch sagt: Das Ankreuzen des Kästchens war eine Entscheidung über routinemäßige Schreibvorgänge, keine Erlaubnis, ein Journal zu löschen.

Endpunkte, die nur *berechnen* (ein Backtest, eine Sharpe Ratio, eine Monte-Carlo-Simulation), fragen nicht nach. Sie ändern nichts, was du vermissen würdest, und ein Bestätigungsdialog bei jeder Berechnung ist genau der Weg, auf dem Leute lernen, Genehmigen zu klicken, ohne zu lesen.

## Werkzeuge über deine Daten {#tools-over-your-data}

Hänge ein **MCP-Token** an eine Unterhaltung, und der Assistent kann deine Module über das [selbe In-Process-Gateway](/de/config/ai-agents) lesen und ändern, das externe MCP-Clients nutzen. Die **Berechtigungsstufen pro Modul** des Tokens (Lesen / Lesen+Schreiben / Voll, unter **Einstellungen → MCP** gesetzt) gelten **unmittelbar**: Das Token *ist* der Berechtigungsrahmen; es gibt kein zweites agentenseitiges Gate. Einstellungen, Geheimnisse, Netzwerk und Datenlösch-Operationen werden nie angeboten, und es gibt konstruktionsbedingt keinen Shell- oder Dateisystemzugriff.

Tool-Aufrufe erscheinen inline als **einklappbare Chips** mit Argumenten und Ergebnis. Ein Lauf ist auf 15 Tool-Runden begrenzt, und jede Unterhaltung trägt ein **Simulationsbudget**, denn Backtests und Sweeps sind das Einzige, was ein Assistent unbegrenzt ausgeben kann. Ist das Budget aufgebraucht, wird ihm gesagt, die Suche zu beenden und zu berichten, was er hat, einschließlich der Zahl der gelaufenen Trials.

### Einen eigenen Skill schreiben

Ein Skill ist ein Verfahren, kein Handbuch. Die Form, die funktioniert:

- eine **Beschreibung**, die sagt, *wann* man danach greift, denn diese Zeile ist der Abrufschlüssel und reist in jedem Prompt mit, „Use when the user proposes a strategy“ ist also besser als „About strategies“;
- ein **Inhalt**, der die genauen Endpunkte Schritt für Schritt benennt, mit den spezifischen Wegen, auf denen die Aufgabe in dieser App schiefgeht;
- ein **Verifikationsschritt**: wie man das Ergebnis belegt, bevor man es meldet;
- eine **Berichtsform**: was die Antwort enthalten muss.

Halte den Inhalt kurz. Er landet beim Laden komplett im Kontextfenster, ein langer verdrängt also die Aufgabe, der er helfen sollte. Der Editor warnt dich ab etwa zweitausend Wörtern.

### Werkzeuge pro Unterhaltung

Jede Unterhaltung trägt **ihr eigenes** MCP-Token (das in den Einstellungen gesetzte ist nur der Standard für neue Unterhaltungen), umschaltbar über ein **Werkzeug-Dropdown** in der Chat-Kopfzeile. Zwei Unterhaltungen können nebeneinander mit unterschiedlichen Datenbereichen laufen. Das Dropdown:

- hat ein **Suchfeld**, um Tokens und externe Server nach Namen zu filtern;
- vermerkt, wenn das gewählte Token **Schreiben/Löschen** gewährt;
- bietet Inline-Aktionen, um einen **MCP-Server hinzuzufügen**, und Schnelllinks zu **Einstellungen → MCP** (Tokens erstellen/verwalten) und zum **MCP-Store**.

## MCP-Store: externe Plattformen verbinden

**Agent → Server verwalten** ist ein Vollseitenbereich zum Hinzufügen entfernter MCP-Server, damit der Assistent externe Plattformen erreichen kann:

- ein **kuratierter Katalog** bekannter Server (DeepWiki, Context7, GitHub, Hugging Face, eigener Schlüssel), plus **eigene Server** per URL;
- **nur Streamable HTTP**: Nichts wird je lokal ausgeführt;
- Authentifizierungswerte sind **im Ruhezustand verschlüsselt und nur schreibbar**;
- eine Schaltfläche **Testen** verbindet und listet die Werkzeuge des Servers;
- aktiviere einen Server pro Unterhaltung über das Werkzeug-Dropdown.

Externe Werkzeuge haben einen **Namensraum** (z. B. `deepwiki__ask_question`) und tragen die Kennzeichnung ihres Servers, Aufrufe sind zeitlich begrenzt, und ein nicht erreichbarer Server **degradiert zu einer Warnung**, statt den Chat zu blockieren.

::: warning Externe Inhalte sind nicht vertrauenswürdig
Ein externer MCP-Server sieht deine Unterhaltung, und was er zurückgibt, sind Inhalte Dritter. Die Kombination eines externen Servers mit einem Token, das **Schreibzugriff** auf deine Daten gewährt, bedeutet, dass eingeschleuste Inhalte versuchen könnten, Änderungen auszulösen, und das Werkzeug-Dropdown warnt dich, wenn diese Kombination aktiv ist. Füge nur Server hinzu, denen du vertraust, und behalte die Tool-Call-Chips im Auge.
:::
