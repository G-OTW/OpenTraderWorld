# Handel aus dem Chart

Ein Broker-Konto kann mehr als Ihr Buch lesen: Mit eingeschaltetem **Trading** sendet der Chart Orders an das Konto, verschiebt und storniert sie über ihre Linie und schließt Positionen über deren Linie. Drei Broker nehmen heute Orders an:

- **Interactive Brokers (Gateway)**, über das IB Gateway oder die TWS, die Sie selbst betreiben. Diese Seite.
- **Binance** und **Binance USDⓈ-M Futures**, auf Echtgeldkonten und in ihren Testumgebungen (Spot Demo Mode, das Futures-Testnet). Ein Echtgeldkonto nimmt Orders an, sobald [Live-Handel](#live-trading) darauf eingeschaltet ist.
- **Paper trading**, ein simuliertes Konto, das die App selbst führt. Siehe [Papierhandel](/de/config/paper-trading).

Jeder andere Broker antwortet, dass er keine Orders von OpenTraderWorld annimmt, und bleibt [nur lesend](/de/config/brokers).

::: danger Experimentell
Der Handel aus dem Chart ist **experimentell**: Er funktioniert vorerst mit Interactive Brokers, Binance und Binance USDⓈ-M Futures. Er ist ausgiebig getestet, gegen simulierte Handelsplätze und die Testumgebungen der Broker, Fehler sind dennoch möglich. Prüfen Sie jede Order beim Broker, beginnen Sie klein und handeln Sie auf eigenes Risiko.
:::

::: tip Echtgeldkonten
Interactive Brokers, Binance und Binance USDⓈ-M Futures sind mit **E4** eingestuft: Ein Echtgeldkonto nimmt Orders an, sobald Sie [Live-Handel](#live-trading) darauf einschalten und seine Limits gesetzt sind. Papierkonten und Testumgebungen brauchen nichts davon.

Das Stornieren von Orders wird nie blockiert, egal bei welchem Konto.
:::

## Einrichten

### 1. Das Gateway

Starten Sie IB Gateway (oder die TWS), angemeldet an Ihrem Papier- oder Echtgeldkonto, und in den API-Einstellungen:

- **Enable ActiveX and Socket Clients** anhaken;
- **Read-Only API** abhaken, sonst lehnt IB selbst jede Order ab;
- mit Docker Engine (Linux) **Allow connections from localhost only** abhaken und `172.28.53.10` zu den **Trusted IPs** hinzufügen.

Die Client-ID wählt die App (3100 bis 3115), mehrere Sitzungen können sich also ein Gateway teilen.

### 2. Das Konto

In **Einstellungen → Broker** auf **Konto hinzufügen**, **Interactive Brokers (Gateway)** wählen und ausfüllen:

| Einstellung | Wert |
|---|---|
| **Gateway host** | `host.docker.internal`, wenn die App in Docker läuft, sonst `127.0.0.1` |
| **Gateway port** | `4002` Papier-Gateway, `4001` Echtgeld-Gateway, `7497` Papier-TWS, `7496` Echtgeld-TWS |

Kein Geheimnis nötig: Das Gateway ist bereits angemeldet. **Verbindung testen** prüft, ob es antwortet.

### 3. Der Trading-Schalter

Jede Kontozeile hat einen **Trading**-Chip neben ihren Modul-Chips. Er ist bewusst getrennt:

- **Alle Module** schließt ihn nicht ein. Ihr Buch lesen und Orders senden sind zwei verschiedene Berechtigungen.
- **Das Einschalten fragt jedes Mal nach Ihrem Passwort.** Das Ausschalten fragt nichts.
- Papierhandelskonten sind die Ausnahme: kein Passwort, da kein echtes Geld im Spiel ist.

### 4. Der Chart

Ein Chart handelt, wenn drei Dinge zusammenpassen:

1. im **Broker book** des Charts ist ein Konto gewählt;
2. Trading ist für dieses Konto eingeschaltet;
3. die Daten des Charts kommen vom selben Broker: Ein IBKR-Konto handelt einen Chart, der vom Interactive-Brokers-Connector gespeist wird.

Fehlt eines davon, sagt das Order-Panel, was und wie es zu beheben ist. Futures brauchen den **Kontraktmonat**: Ein fortlaufender Future wird abgelehnt, öffnen Sie also `MNQZ6` statt des fortlaufenden Charts.

## Die Trading-Sperre {#trading-lock}

Ein optionaler zweiter Faktor für Orders. Mit aktiver Sperre fragt die erste Order nach der Anmeldung einmal nach dem Code aus Ihrer Authenticator-App, danach bleiben alle Trading-Konten in diesem Browser bis zur Abmeldung offen.

- Sie schützt Platzieren, Ändern und Schließen. Stornieren, einzeln oder alle, fragt nie.
- Sie gilt für alle Trading-Konten außer [Papierkonten](/de/config/paper-trading).
- Einschalten unter **Einstellungen → Sicherheit → Trading-Sperre**, sobald die Zwei-Faktor-Authentifizierung eingerichtet ist. Details, falsche Codes und Zurücksetzen: [Kontosicherheit](/de/config/security#trading-lock).

::: danger Zwei-Faktor bei aktiver Sperre entfernen schaltet Trading überall ab
Das Entfernen der Zwei-Faktor-Authentifizierung, in den Einstellungen oder mit `disable-totp`, schaltet die Sperre ab. War die Sperre aktiv, schaltet es zudem das Trading auf **allen** Konten aus, Papierkonten eingeschlossen. Schalten Sie es danach pro Konto wieder ein.
:::

## Orders senden

### Das Ticket

Der Tab **Ticket** des Order-Panels sendet eine Order, eine Bracket-Order oder eine OCO. **Er bietet an, was der Broker des Kontos annimmt, und nichts darüber hinaus**: Ein Typ, eine Gültigkeit oder eine Option, die der Broker nicht angibt, wird nicht gezeigt (siehe [Was jeder Broker annimmt](#what-each-broker-takes)).

- **Typen:**
  - **Market**, **Limit**, **Stop**, **Stop limit**.
  - **Trailing stop**: ein Stop, der dem Markt in einem Abstand folgt, als Prozentsatz oder Preisbetrag, und nie zurückweicht. **Start at** lässt ihn, wo der Broker es hat, auf einen Preis warten, bevor er zu folgen beginnt.
  - **MIT** (market if touched) und **LIT** (limit if touched): das Spiegelbild eines Stops, ausgelöst, wenn der Markt von der Limit-Seite her den Preis erreicht (ein Kauf unter dem Markt, ein Verkauf darüber), dann eine Market- oder eine Limit-Order.
  - **OCO**: ein Limit und ein Stop auf derselben Seite, wobei die erste Ausführung die andere storniert. Die beiden Preise müssen sich unterscheiden.
- **Ausstiege:** Die Schalter **Take profit** und **Stop loss** fügen die Gegenorder über dieselbe Menge hinzu. Beim Einschalten wird 0,5 % (Take profit) bzw. 0,25 % (Stop loss) vom Einstieg vorbelegt. **Exits time in force** wählt GTC (Standard) oder Day. Wo der Broker Reduce-only-Orders annimmt, werden Ausstiege als Reduce-only gesendet. Nicht verfügbar bei OCO und bei einem Broker, der keine Ausstiege anhängt. Bei Interactive Brokers wird der Einstieg zurückgehalten, bis seine Ausstiege angenommen sind, die Bracket geht also vollständig raus.
- **Gültigkeit:** Day, GTC, GTD mit Datum und Uhrzeit in der Zukunft, IOC, FOK (alles oder nichts, sofort).
- **Preise:** als Preis oder als Abstand zu Bid, Ask oder Last (zum Einstieg bei Ausstiegen), in Ticks, wenn der Tick bekannt ist.
- **Größe:** in **Einheiten** (Aktien, Kontrakte), als **Betrag** in Geld (geteilt durch den Wert des Einstiegs) oder als Geld, das bis zum Stop loss **riskiert** wird (geteilt durch den Abstand vom Einstieg zum Stop loss, der eingeschaltet sein muss). Ein Future zählt seinen Punktwert; Kontrakte und Aktien werden auf ganze Einheiten abgerundet.
- **Optionen**, jeweils nur gezeigt, wo der Broker sie annimmt:
  - **Reduce only**: verkleinert nur die Position, eröffnet oder dreht nie eine.
  - **Post only**: ein Limit, das nur im Orderbuch ruht; abgelehnt, statt sofort ausgeführt zu werden.
  - **Iceberg**: die im Orderbuch gezeigte Größe, der Rest verborgen.
  - **Trigger on**: der Preis, den ein Stop oder Trigger beobachtet (Last, Mark, Bid/Ask, Mid).
  - **Close position**: ein Stop oder MIT, der bei seinem Trigger die ganze Position schließt, gleich welcher Größe.
  - **Outside RTH**: löst auch außerhalb der regulären Handelszeiten aus und wird dort ausgeführt.
  - **Adaptive algo**: der Broker arbeitet eine Market- oder Limit-Order für einen besseren Preis ab, geduldig, normal oder dringend.

Bevor etwas rausgeht, prüft die App:

- jeder Preis und jede Menge ist größer als null, und jede Option passt zum Typ (Post-only auf einem Limit, ein Iceberg unter der Ordergröße, ein Trailing-Prozentsatz unter 100);
- der Broker nimmt den Typ, die Gültigkeit und jede gewünschte Option an: Was er nicht annimmt, wird abgelehnt und benannt;
- ein Preis außerhalb des Ticks wird mit den zwei nächsten gültigen Preisen abgelehnt (`… is not on MNQZ6's 0.25 tick: use … or …`);
- das Instrument wird beim Broker aufgelöst. **Es wird nie geraten.**

Wird ein Teil eines Tickets abgelehnt, wird das ganze Ticket storniert.

### Orders, denen die App folgt {#followed-orders}

Jede Order wird **vor** dem Senden unter ihrer eigenen ID gespeichert, und die Antwort des Brokers wird mit ihr gespeichert. Daher gilt:

- **Eine Order-ID wird pro Konto nur einmal gesendet, für immer.** Der Chart gibt jeder Order eine neue ID; ein erneut gesendetes Ticket (etwa nach Angabe einer Margin) geht unter neuen IDs raus. Eine bereits verwendete ID wird abgelehnt, auch nach einem Neustart.
- **Keine Antwort ist keine Ablehnung.** Eine Order, die der Broker nicht rechtzeitig bestätigt (10 Sekunden beim Gateway), wird beim Broker anhand ihrer ID nachgeschlagen. Ist sie noch nicht zu finden, wird sie als **unbekannt** markiert und **nie erneut gesendet**: Neue Orders auf diesem Instrument warten, bis eine Abfrage sie klärt. Ist sie nach 5 Minuten noch ungeklärt, erhalten Sie eine Benachrichtigung (Benachrichtigungskanal des Charts), die Sie bittet, die Order beim Broker zu prüfen.
- **Eine Order, die der Broker nie erhalten hat** (nach den Abfragen weiterhin nicht zu finden), wartet auf Sie. Der Tab Orders zeigt *the broker never received this order* mit **Send again**, das sie unter neuen IDs durch jede Prüfung eines neuen Tickets sendet, und **Dismiss**, das nichts sendet. Sie erhalten außerdem eine Benachrichtigung.
- **Ein Neustart vergisst nichts.** Orders, die kurz vor dem Stopp der App gesendet wurden, werden beim Start nachgeschlagen, und Orders, die die App selbst verknüpft (Binance-USDⓈ-M-Brackets), werden wieder aufgenommen.

Danach gleicht die App ihre Sicht mit der des Brokers ab, einige Sekunden nach dem Start und zyklisch (jede Minute, solange sie Orders draußen hat, sonst alle 10 Minuten, und sofort, wenn ein Live-Stream verpasste Ereignisse meldet). **Der Broker hat recht**: Eine Order, die ausgeführt oder storniert wurde, während niemand hinsah, übernimmt den Status des Brokers, und eine anderswo (TWS, die Website der Börse) platzierte Order wird als extern vermerkt. Eine Position, die sich ohne erklärende Ausführungen geändert hat, ist eine **Drift**: Sie wird im Audit-Log festgehalten und gemeldet.

### Was jeder Broker annimmt {#what-each-broker-takes}

| | Interactive Brokers | Binance Spot | Binance USDⓈ-M | Paper |
|---|---|---|---|---|
| **Typen** | alle | alle | alle | alle |
| **Gültigkeit** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK |
| **Ausstiege, OCO** | von IB verknüpft | Orderlisten (OTO, OTOCO, OCO) | von der App verknüpft | von der App verknüpft |
| **Trailing stop** | Betrag oder %, ab dem Markt | %, in Schritten von 0,01 %, Startpreis | % (0,1 bis 20), Startpreis | Betrag oder %, Startpreis |
| **Reduce only** | | | ja | ja |
| **Post only** | | ja (Limit Maker) | ja (GTX) | ja |
| **Iceberg** | ja, ganze Einheiten | ja, GTC-Limits | | |
| **Trigger on** | Last, Bid/Ask, Mid | | Last, Mark | |
| **Close position** | | | ja | |
| **Outside RTH** | ja | | | |
| **Adaptive algo** | ja | | | |

- **Binance Spot** hält Ausstiege für einen **Limit**-Einstieg zurück (OTO, OTOCO) und platziert die eines **Market**-Einstiegs, sobald dieser ausgeführt ist; ein Stop-Einstieg nimmt keine Ausstiege an. Die Limit-Hälfte einer OCO geht als Limit Maker raus. Ein Verkaufsausstieg lautet auf die gekaufte Menge: Eine in der Coin selbst abgezogene Gebühr lässt weniger übrig, und Binance lehnt den Ausstieg ab.
- **Binance USDⓈ-M** hängt selbst nichts an: Die App sendet Ausstiege als Reduce-only-Orders neben ihrem Einstieg und verknüpft sie sowie OCO-Paare, indem sie sie alle 5 Sekunden ausliest. Nach einem Neustart nimmt die App die Verknüpfungen aus den gespeicherten Orders wieder auf. Der Hedge-Modus wird befolgt: Jede Order nennt ihre Long- oder Short-Seite, und ein in beide Richtungen gehaltenes Symbol wird Seite für Seite geschlossen.
- **Interactive Brokers**-Trailing-Stops folgen ab dem Moment der Platzierung (kein Startpreis) und zeigen einen Iceberg in ganzen Einheiten.

### Binance

- **API-Schlüssel.** Orders brauchen einen Schlüssel mit aktiviertem Trading („Enable Spot & Margin Trading“ für Spot, „Enable Futures“ für USDⓈ-M) und deaktivierten Auszahlungen. Bei einem Live-Konto liest die App vor der ersten Order einer Sitzung die Rechte des Schlüssels und sendet nicht, wenn Trading aus ist, Auszahlungen an sind oder der Schlüssel nicht auf vertrauenswürdige IPs beschränkt ist. Danach führt sie die Order als Probelauf auf der Testroute von Binance aus.
- **Prüfungen vor dem Senden.** Jede Order wird an die Filter des Symbols gehalten, wie Binance sie veröffentlicht: Tick, Größenschritt und -grenzen, Größe von Market-Orders, minimaler und maximaler Ordernennwert, das Preisband um den aktuellen Referenzpreis (Mark Price bei Futures) und die Obergrenze offener Orders pro Symbol. Eine abgelehnte Order nennt den Filter und die nächsten Werte, die durchgehen würden. Nichts wird für Sie geändert.
- **Order-Rate.** Würde das Order-Limit des Kontos überschritten, wird die Order mit der Anzahl der zu wartenden Sekunden abgelehnt. Sie wird nie in eine Warteschlange gestellt.
- **Futures-Stop ändern.** Binance kann eine bedingte Order nicht an Ort und Stelle ändern. Die App platziert zuerst den geänderten Stop und storniert dann den alten, sodass immer ein Stop aktiv ist. Die Order erhält eine neue ID (`….r1`).
- **Wenn die App wegfällt (USDⓈ-M).** Solange sie Orders auf einem Futures-Symbol aktiv hat, hält die App den Auto-Cancel-Countdown von Binance scharf (120 s, alle 30 s erneuert). Hört sie auf, ihn zu erneuern, storniert Binance alle offenen Orders auf diesem Symbol, auch anderswo platzierte.
- **Positionen.** Futures-Positionen zeigen den bei Binance eingestellten Hebel und Margin-Modus (Cross oder Isolated). Die App liest sie nur.
- **Tab Konto (Spot).** Cash ist der USDT-Saldo; verfügbar ist sein freier Teil. Equity ist Binances eigener Wert des Spot-Wallets in USDT, nur bei live (Spot Demo Mode veröffentlicht ihn nicht).

### Interactive Brokers

- Eine Größe, die der Kontrakt nicht annimmt (unter seinem Minimum oder neben seinem Größenschritt, zum Beispiel 0,5 eines Futures), wird vor dem Senden mit den gültigen Größen abgelehnt.
- Ein Storno, das nach der Ausführung der Order eintrifft, ist kein Fehler: Die Order wird als ausgeführt angezeigt.
- Solange IB meldet, seine Server verloren zu haben (1100), werden neue Orders und Änderungen abgelehnt; Stornos gehen weiterhin durch.
- Bei einem Live-Konto geht die erste Order jeder Gateway-Verbindung zuerst als What-if an IB. Lehnt IB sie ab oder meldet, dass das Konto sie nicht tragen kann, wird das Ticket mit der Begründung von IB abgelehnt.

Das Ticket dockt unten, oben, links oder rechts am Chart an, oder zwischen Zeilen oder Spalten eines Layouts. Ziehen Sie seine Innenkante, um die Größe zu ändern (Pfeiltasten gehen auch, Shift für größere Schritte); die Größe wird pro Andockposition gemerkt.

### Die Schnellleiste

Die **Schnell-Kauf/Verkauf-Leiste** (Schalter in der Werkzeugleiste) zeigt Verkaufen, eine Menge und Kaufen mit Bid und Ask. **Ein Klick sendet sofort eine Market-Order, ohne Bestätigung.** Ziehen Sie sie beliebig; sie rastet am nächsten Rand ein.

### Das `+` an der Preisskala

Fahren Sie über die Preisskala und klicken Sie auf **+**: Das Menü bietet einen Kauf oder Verkauf zu diesem Preis an, als Limit oder Stop, je nachdem, auf welcher Seite des Marktes er liegt. Die Order geht sofort raus, als Day, oder öffnet sich mit **Im Ticket öffnen…** im Ticket.

## Im Chart

- **Aktive Orders** werden als Linien gezeichnet. Ziehen Sie das Label einer Linie, um die Order zu verschieben (sie rastet am Tick ein); eine abgelehnte Verschiebung setzt die Linie zurück. Ihr **✕** storniert die Order und ihre Ausstiege.
- **Doppelklicken** Sie eine Order-Linie, oder nutzen Sie den Tab Orders, um sie im Ticket zu öffnen und Preise, Menge oder Gültigkeit zu ändern. Seite und Typ lassen sich nicht ändern. In der TWS platzierte Orders erscheinen ebenfalls und lassen sich genauso bearbeiten, wenn es Market-, Limit-, Stop- oder Stop-Limit-Orders sind.
- **Positionen** werden auf ihrem durchschnittlichen Preis gezeichnet, mit einem **✕** zum Schließen zum Marktpreis. Das Schließen sendet eine Market-Order über die Menge, die der Broker jetzt hält, und **storniert dann die Ausstiege der Position**: die aktiven Orders auf ihrem Instrument, auf der Schließseite, die der Broker mit einem Einstieg verknüpft oder als Reduce-only markiert. Andere Orders auf dem Instrument bleiben.
- **Mit Ausstiegen absichern** (das Schild im Tab Positionen) öffnet das Ticket auf dem Chart der Position mit einer OCO auf der Gegenseite über die gehaltene Menge: ein Take profit und ein Stop loss, die einander stornieren, Reduce-only, wo der Broker es hat. Die Position muss auf einem Chart dieses Kontos offen sein.
- **Brackets:** Fährt man über eine Order einer Bracket, werden ihre Verbindungen gezeichnet, grün gestrichelt zum Take profit und rot gestrichelt zum Stop loss.
- **Fills:** Mit **Entry and exit markers** werden die Fills des Kontos auf dem Instrument als Markierungen auf ihrem Preis und Balken gezeichnet (ein Kauf zeigt nach oben, ein Verkauf nach unten), über höchstens die letzten 30 Tage. Standardmäßig aus.

**Chart-Einstellungen → Trading** legt die Darstellung fest:

| | Optionen |
|---|---|
| **Position** | Ergebnis auf der Linie (keins, Prozent, Betrag), Linie (volle Breite, kurz, nur Markierung an der Preisskala), Seite anzeigen, Menge anzeigen |
| **Order** | Linie, Seite anzeigen, Menge anzeigen |
| **Fills** | Entry and exit markers, Farben von Buy marker und Sell marker |

## Das Order-Panel

Neben **Ticket**, nach Broker gefiltert:

| Tab | Zeigt |
|---|---|
| **Positionen** | Symbol, Seite, Menge, Durchschnittspreis, Marktpreis, P&L, Eröffnungszeit und Einstiegsgebühren (aus den Ausführungen der letzten 7 Tage), mit Schließen-Button |
| **Orders** | Typ, Menge, Limit, Stop und Status, mit Bearbeiten und Stornieren. Sein **Alle stornieren** storniert die in diesem Browser gelisteten Orders |
| **Konto** | die Kennzahlen, die der Broker angibt, siehe unten |
| **Logs** | die letzten 200 an den Broker gesendeten Aufrufe, aus dem Audit-Log |

### Kontokennzahlen

**Equity, Cash, Offenes Ergebnis, Realisiert, Initial Margin, Maintenance Margin, Verfügbar, Überschussliquidität, Kaufkraft, Positionswert**, in der Basiswährung des Kontos, eine Zeile pro Konto, das das Gateway erreicht. Bei Interactive Brokers sind es die Werte von IB (`NetLiquidation`, `TotalCashValue`, `UnrealizedPnL`, der heutige `RealizedPnL`, `InitMarginReq`, `MaintMarginReq`, `AvailableFunds`, `ExcessLiquidity`, `BuyingPower`, `GrossPositionValue`). Eine Kennzahl, die der Broker nicht angibt, wird nicht gezeigt: IB nennt keine Provisionssumme, diese Zeile fehlt also.

### Live-Aktualisierung

Orders, Ausführungen und Positionen treffen so ein, wie der Broker sie meldet, über einen Stream für alle Trading-Konten der Seite: die eigenen Meldungen des Gateways bei Interactive Brokers, der User Data Stream von Binance bei Binance (die WebSocket-API bei Spot, der Listen Key des Kontos bei USDⓈ-M, beide mit dem vorhandenen Schlüssel des Kontos). Hedge-Mode-Futures-Positionen werden vollständig neu gelesen, statt an Ort und Stelle aktualisiert zu werden. Das Buch wird außerdem alle 30 Sekunden neu gelesen und eine Sekunde nach jeder Positionsänderung. Fällt der Stream aus, liest die Seite das Buch alle 4 Sekunden. Ein getrenntes Gateway wird beim nächsten Aufruf neu verbunden; aktive Orders bleiben in der Zwischenzeit bei IB.

## Risikolimits und Notschalter {#risk-limits}

Ihre eigenen Limits, geprüft, bevor eine Order rausgeht, auf jedem Trading-Konto (Paper eingeschlossen). Jedes ist aus, bis Sie es setzen, unter **Einstellungen → Broker → Risikolimits** des Kontos:

| Limit | Lehnt ab |
|---|---|
| **Größte Order (Größe)** | eine Order über dieser Stückzahl |
| **Größter Orderwert (Kontowährung)** | eine Order, die mehr wert ist, in der Kontowährung (Größe × Preis × Punktwert eines Futures) |
| **Größte Position (Größe)** | eine Order, nach der mehr als diese Stückzahl im Instrument gehalten würde |
| **Größter Positionswert (Kontowährung)** | eine Order, nach der die Position mehr wert wäre, in der Kontowährung (gehaltene Größe × Preis × Punktwert eines Futures) |
| **Preiskorridor (%)** | ein Limit oder Stop, der mehr als diesen Prozentsatz vom letzten Preis des Charts entfernt liegt |
| **Orders pro Minute** | mehr Orders als diese Zahl in der letzten Minute |
| **Gleichzeitig offene Orders** | mehr aktive Orders als diese Zahl |
| **Verlustlimit pro Zeitraum (Kontowährung)** | alles, sobald das Konto seit Beginn des Zeitraums (Stunde, Tag oder Woche, siehe unten) so viel verloren hat: das Konto wird angehalten |
| **Duplikatfenster (Sekunden)** | dieselbe Order (Seite, Typ, Größe, Preise), die innerhalb dieser Sekunden erneut gesendet wird, bis Sie bestätigen |

- Eine Verletzung wird mit Limit und Wert abgelehnt. **Die Order wird nie verkleinert, damit sie passt.**
- **Risiko abbauen geht immer durch**: eine Reduce-only-Order, ein Schließen oder eine Order, die nur das Gehaltene verkleinert, passiert jedes Limit und jeden Halt.
- Ein Limit zu ändern verlangt Ihr Passwort.

### Halts

Ein Halt lehnt neue Orders ab, bis er endet; Stornierungen und Orders, die eine Position verkleinern, gehen weiterhin raus, und aktive Orders lassen sich in der Zwischenzeit nicht ändern (stornieren Sie sie stattdessen).

- **Notschalter**: der Button unter **Einstellungen → Broker** und im Order-Panel des Charts storniert alle aktiven Orders auf **jedem** Trading-Konto (anderswo platzierte Orders eingeschlossen, jede Stornierung im Audit-Log) und hält dann alle an. Er verlangt eine Bestätigung, nie ein Passwort.
- **Verlustlimit**: Ein Konto, das sein Verlustlimit erreicht, hält sich selbst an (geprüft vor jeder Order und im Abgleichzyklus), und Sie erhalten eine Benachrichtigung.
- **Positionsabweichung**: Bewegt sich eine Position ohne Fills, die das erklären (eine Order, die die App nicht gesehen hat, ein korrigierter Fill), hält sich das Konto an, bis Sie es **bestätigen**, und Sie erhalten eine Benachrichtigung. Prüfen Sie zuerst das Konto beim Broker; das Bestätigen verlangt immer Ihr Passwort, unabhängig von der Richtlinie unten.

Unter **Einstellungen → Broker → Risiko und Notschalter** wählen Sie für alle Konten, wie ein Halt endet:

- **nach**: Beginn der nächsten Stunde, des nächsten Tages oder der nächsten Woche (Montag) in der **Zeitzone** der Regel; oder **nur wenn aufgehoben**. Die Zone ist UTC, bis Sie eine speichern, und das Formular startet mit der Zone Ihres Browsers;
- **früher mit meinem Passwort aufheben**: Ein Halt kann vorzeitig am selben Ort oder über das Banner des Charts aufgehoben werden. Ein Halt, der nur durch Aufheben endet, wird immer mit dem Passwort aufgehoben.

Das Speichern der Regel verlangt Ihr Passwort.

Der Verlustzeitraum folgt derselben Wahl (ein Tag, wenn Halts nur durch Aufheben enden). Der Verlust wird ab dem Eigenkapital des Kontos zu Beginn des Zeitraums gemessen, wie es der Broker angibt; eine Einzahlung oder Auszahlung im Zeitraum zählt mit. Ein Broker, der kein Eigenkapital angibt (Binance Spot Demo Mode), kann an kein Verlustlimit gebunden werden: Seine Orders werden abgelehnt, solange eines gesetzt ist.

## Live-Handel {#live-trading}

Die Orderstrecke jedes Brokers hat eine Stufe, die am Konto unter **Einstellungen → Broker** angezeigt wird: E2 (gegen einen simulierten Handelsplatz getestet), E3 (über Tage in der eigenen Testumgebung des Brokers getestet), E4 (ein Live-Probelauf und eine Live-Order in kleinster Größe sind bestanden). Unterhalb von E4 wird ein Konto, das echtes Geld handelt, vom Connector selbst abgelehnt.

Bei E4 nimmt ein Live-Konto weiterhin keine Order an, bis Sie darauf **Live-Handel** einschalten:

- Voraussetzung sind der eingeschaltete Trading-Schalter sowie das **Größen- oder Wertlimit pro Order**, das **Größen- oder Wertlimit pro Position** und das **Verlustlimit** des Kontos;
- die [Trading-Sperre](#trading-lock) ist Ihre Wahl. Ist sie aktiv, fragen das Einschalten des Live-Handels und die erste Order jeder Anmeldung nach dem Code Ihrer Authenticator-App. Ist sie aus, zeigt das Konto einen Hinweis und seine Orders gehen ohne Code raus;
- das Konto zeigt die Stufe, ob der Broker Ihre Orders storniert, wenn die App stoppt (Binance USDⓈ-M: ja; Spot und Interactive Brokers: nein, die Orders bleiben aktiv), und welche Limits gesetzt sind;
- das Ausschalten fragt nichts; wird das Trading ausgeschaltet, schaltet sich auch dieser Schalter aus und muss neu eingeschaltet werden.

Testumgebungen, Demokonten und Papierkonten brauchen ihn nie.

## Audit-Log und Order-Kennzahlen {#audit-log}

Jedes Platzieren, Ändern, Stornieren, Alles-Stornieren und Schließen wird in ein **nur anfügbares Audit-Log** geschrieben: Zeit, Konto, Broker, Aktion, Anfrage, Antwort oder Fehler des Brokers und Latenz. Die Datenbank verweigert das Ändern oder Löschen einer Zeile. Aufrufe, die vor dem Broker abgelehnt werden, weil Trading aus oder die Sperre geschlossen ist, werden nicht protokolliert; jede andere Ablehnung schon. Was der [Abgleich](#followed-orders) findet (eine verpasste Ausführung oder Stornierung, eine externe Order, eine nach verlorener Antwort geklärte Order, Positionsdrift), wird ebenfalls protokolliert, als **reconcile**, und nicht in den Order-Kennzahlen mitgezählt.

In **Einstellungen → Broker** zeigt jedes Trading-Konto seine **Order-Kennzahlen** (Aufrufe, platziert, abgelehnt, Latenz p50 / p99, letzte Order) und einen Button **Alle Orders stornieren**. Anders als der [Notschalter](#halts) gilt er nur für dieses eine Konto und hält nichts an:

- er fragt nach Bestätigung und storniert dann **jede aktive Order** des Kontos beim Broker, außerhalb von OpenTraderWorld platzierte Orders eingeschlossen. Bei Interactive Brokers sind das alle Orders der Konten des Gateways, einschließlich von Hand in der TWS platzierter Orders;
- er meldet, wie viele storniert wurden. Bei Interactive Brokers wartet er zuvor bis zu 10 Sekunden, bis keine mehr aktiv ist;
- er funktioniert bei ausgeschaltetem Trading und geschlossener Sperre.

## Grenzen

- Bei Interactive Brokers gehen Orders an das Standardkonto des Gateways.
- Keine Änderung von Seite oder Typ einer aktiven Order: stornieren und neu senden.
- Orders lauten auf die Währung des Kontrakts, ohne Umrechnung.
- KI-Agenten ([MCP](/de/config/ai-agents)) und Automator-Workflows können keine Orders senden.
- Broker-Konten, und damit das Trading, sind im [Demo-Modus](/de/guide/demo) deaktiviert.
