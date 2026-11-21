# Handel aus dem Chart

Ein Broker-Konto kann mehr als Ihr Buch lesen: Mit eingeschaltetem **Trading** sendet der Chart Orders an das Konto, verschiebt und storniert sie über ihre Linie und schließt Positionen über deren Linie. Diese Broker nehmen heute Orders an:

- **Interactive Brokers (Gateway)**, über das IB Gateway oder die TWS, die Sie selbst betreiben. Diese Seite.
- **Binance** und **Binance USDⓈ-M Futures**, auf Echtgeldkonten und in ihren Testumgebungen (Spot Demo Mode, das Futures-Testnet). Ein Echtgeldkonto nimmt Orders an, sobald [Live-Handel](#live-trading) darauf eingeschaltet ist.
- **Bybit** (Spot- oder Linear-Book), auf Echtgeldkonten und im Bybit-Testnet. Eingestuft **E2D**: nach der Dokumentation von Bybit gebaut und nie gegen Bybit getestet; ein Echtgeldkonto nimmt Orders an, sobald [Live-Handel](#live-trading) darauf mit der Bestätigung eingeschaltet ist.
- **Bybit EU** (Spot), auf Echtgeldkonten und im Bybit-EU-Testnet. Eingestuft **E2D** wie Bybit, mit derselben Bestätigung für ein Echtgeldkonto.
- **OKX** (Spot, Perpetual Swaps und Futures mit Laufzeit), auf Echtgeldkonten und im OKX-Demohandel. Eingestuft **E2D**, mit derselben Bestätigung für ein Echtgeldkonto.
- **Bitget** (Spot und USDT-M-Futures), auf Echtgeldkonten und im Bitget-Demohandel. Eingestuft **E2D**.
- **Gate.io** (Spot und USDT-Perpetuals), auf Echtgeldkonten und im Gate.io-Testnet. Eingestuft **E2D**.
- **KuCoin** (Spot) und **KuCoin Futures**, nur auf Echtgeldkonten: KuCoin hat keine Testumgebung. Eingestuft **E2D**.
- **Kraken** (Spot), nur auf Echtgeldkonten: Kraken hat keine Spot-Testumgebung. Eingestuft **E2D**.
- **Kraken Futures** (Multi-Collateral-Kontrakte, PF_ und FF_), auf Echtgeldkonten und in der Kraken-Futures-Demo. Eingestuft **E2D**, mit derselben Bestätigung für ein Echtgeldkonto.
- **Coinbase Advanced Trade** (Spot), nur auf Echtgeldkonten: Coinbase hat kein Testkonto. Eingestuft **E2D**.
- **MEXC** (Spot und USDT-Perpetuals), nur auf Echtgeldkonten: MEXC hat keine Testumgebung. Eingestuft **E2D**.
- **Bitfinex** (Spot-Exchange-Wallet und Perpetuals), nur auf Echtgeldkonten: Bitfinex hat keinen Testhost. Eingestuft **E2D**.
- **Deribit** (Futures und Perpetuals: lineare USDC-Kontrakte und inverse BTC- und ETH-Kontrakte), auf Echtgeldkonten und auf test.deribit.com. Eingestuft **E2D**, mit derselben Bestätigung für ein Echtgeldkonto.
- **Paper trading**, ein simuliertes Konto, das die App selbst führt. Siehe [Papierhandel](/de/config/paper-trading).

Jeder andere Broker antwortet, dass er keine Orders von OpenTraderWorld annimmt, und bleibt [nur lesend](/de/config/brokers).

::: danger Experimentell
Der Handel aus dem Chart ist **experimentell**: Er funktioniert mit Interactive Brokers, Binance, Binance USDⓈ-M Futures, Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex und Deribit sowie mit dem Papierhandel. Interactive Brokers und die beiden Binance-Konten sind ausgiebig getestet, gegen simulierte Handelsplätze und die Testumgebungen der Broker. Die anderen sind nach der Dokumentation des jeweiligen Brokers gebaut und wurden noch nicht an einem echten Konto ausprobiert. Fehler sind bei allen möglich. Prüfen Sie jede Order beim Broker, beginnen Sie klein und handeln Sie auf eigenes Risiko.
:::

::: tip Echtgeldkonten
Interactive Brokers, Binance und Binance USDⓈ-M Futures sind mit **E4** eingestuft: Ein Echtgeldkonto nimmt Orders an, sobald Sie [Live-Handel](#live-trading) darauf einschalten und seine Limits gesetzt sind. Papierkonten und Testumgebungen brauchen nichts davon.

Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex und Deribit sind mit **E2D** eingestuft: ein Testnet- oder Demokonto nimmt sofort Orders an; ein Echtgeldkonto braucht Live-Handel mit der Bestätigung, dass der Broker nie gegen die Plattform getestet wurde (siehe [Live-Handel](#live-trading)).

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

| | Interactive Brokers | Binance Spot | Binance USDⓈ-M | Paper | Bybit | Bybit EU | OKX | Bitget | Gate.io | KuCoin | Kraken | Kraken Futures | Coinbase | MEXC | Bitfinex | Deribit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **Typen** | alle | alle | alle | alle | Spot: Market, Limit; Linear: Market, Limit, Stop, Stop-Limit | Market, Limit | Market, Limit | Spot: Market, Limit, Stop; Futures: Market, Limit, Stop, Stop-Limit | Market, Limit | Spot: Market, Limit; Futures: Market, Limit, Stop, Stop-Limit | Market, Limit, Stop, Stop-Limit, MIT, LIT | Market, Limit, Stop, Stop-Limit, MIT, LIT | Market, Limit, Stop-Limit, LIT | Market, Limit | Market, Limit, Stop, Stop-Limit | Market, Limit, Stop, Stop-Limit, MIT, LIT |
| **Gültigkeit** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK (eine Market-Order läuft IOC: FOK abgelehnt) | GTC, IOC, FOK (eine Market-Order läuft IOC: FOK abgelehnt) | GTC, IOC, FOK (FOK bei einer Market-Order abgelehnt) | GTC, IOC, FOK (FOK bei einer Market-Order abgelehnt; ein Stop ist GTC) | GTC, IOC, FOK (eine Market-Order arbeitet IOC; Futures: FOK bei einer Market-Order abgelehnt) | GTC, IOC, FOK (FOK nur bei Limit) | GTC, GTD, IOC, FOK (FOK nur bei Limit; IOC bei einer Limit- oder Market-Order) | GTC, IOC, FOK (eine Market-Order läuft IOC: FOK abgelehnt; Stops, MIT und LIT GTC) | GTC, GTD, IOC, FOK (eine Market-Order läuft IOC: FOK und GTD abgelehnt; ein Stop-Limit ist GTC oder GTD) | GTC, IOC, FOK (FOK nur bei Limit) | GTC, GTD, IOC, FOK (IOC und FOK nur bei Limit; ein Stop ist GTC oder GTD) | GTC, Day, IOC, FOK (Day bei einer Market-Order abgelehnt) |
| **Ausstiege, OCO** | von IB verknüpft | Orderlisten (OTO, OTOCO, OCO) | von der App verknüpft | von der App verknüpft |  Linear: Take-Profit und Stop-Loss auf die ganze Position, zum Marktpreis | | | Futures: Take-Profit und Stop-Loss einer Position, im Order-Panel verschoben oder entfernt | | | |  | |  | |  |
| **Trailing stop** | Betrag oder %, ab dem Markt | %, in Schritten von 0,01 %, Startpreis | % (0,1 bis 20), Startpreis | Betrag oder %, Startpreis | | | | | | | |  | |  | |  |
| **Reduce only** | | | ja | ja | Linear | | Swaps und Futures | Futures | Futures | Futures | | ja | | Futures | Perpetuals | ja |
| **Post only** | | ja (Limit Maker) | ja (GTX) | ja | | | ja | ja | | ja | ja | ja | ja | ja | ja | ja |
| **Iceberg** | ja, ganze Einheiten | ja, GTC-Limits | | | | | | | | | |  | |  | |  |
| **Trigger on** | Last, Bid/Ask, Mid | | Last, Mark | | letzter (Linear-Stops) | | | Spot: letzter; Futures: letzter, Mark | | letzter (Futures-Stops) | letzter | letzter, Mark, Index | letzter | |  | letzter, Mark, Index (bei jedem Stop angegeben) |
| **Close position** | | | ja | | | | | | | | |  | |  | |  |
| **Outside RTH** | ja | | | | | | | | | | |  | |  | |  |
| **Adaptive algo** | ja | | | | | | | | | | |  | |  | |  |

- **Binance Spot** hält Ausstiege für einen **Limit**-Einstieg zurück (OTO, OTOCO) und platziert die eines **Market**-Einstiegs, sobald dieser ausgeführt ist; ein Stop-Einstieg nimmt keine Ausstiege an. Die Limit-Hälfte einer OCO geht als Limit Maker raus. Ein Verkaufsausstieg lautet auf die gekaufte Menge: Eine in der Coin selbst abgezogene Gebühr lässt weniger übrig, und Binance lehnt den Ausstieg ab.
- **Binance USDⓈ-M** hängt selbst nichts an: Die App sendet Ausstiege als Reduce-only-Orders neben ihrem Einstieg und verknüpft sie sowie OCO-Paare, indem sie sie alle 5 Sekunden ausliest. Nach einem Neustart nimmt die App die Verknüpfungen aus den gespeicherten Orders wieder auf. Der Hedge-Modus wird befolgt: Jede Order nennt ihre Long- oder Short-Seite, und ein in beide Richtungen gehaltenes Symbol wird Seite für Seite geschlossen.
- **Interactive Brokers**-Trailing-Stops folgen ab dem Moment der Platzierung (kein Startpreis) und zeigen einen Iceberg in ganzen Einheiten.

### Bybit

- **Jede Order geht an das Book ihres Instruments.** Ein Bybit-Konto zeigt seine Orders und Positionen aus jedem Book, das es liest. Eine Order geht an das Book des Chart-Instruments: das, auf das der Bybit-Datenconnector des Charts eingestellt ist (sein **Market**: spot oder linear), sonst an das einzige Book, das das Symbol führt. Ein Symbol, das beide Books führen (`BTCUSDT`, Spot-Paar und Perpetual), auf einem Chart mit Daten von anderswo wird mit dieser Korrektur abgelehnt. Das Ticket bietet an, was dieses Book annimmt.
- **Testnet.** Setzen Sie die **Site** des Kontos auf `testnet` (ein Schlüssel von testnet.bybit.com): seine Orders bewegen kein Geld und brauchen keinen Live-Handel.
- **Prüfungen vor dem Senden.** Tick, Größenschritt, Mindest- und Höchstgröße (Limit und Market getrennt) und Mindestwert der Order, aus der Instrumentenliste von Bybit. Eine abgelehnte Order nennt die Regel und die nächsten gültigen Werte; nichts wird geändert. Ein Instrument, das gerade nicht gehandelt wird (Pre-Launch, in Lieferung, geschlossen), nimmt keine neue Order an; Stornierungen gehen weiter raus.
- **Positionsmodus.** One-Way- oder Hedge-Modus wird von Ihrem Bybit-Konto gelesen (einmal, dann über den Stream von Bybit aktuell gehalten) und nie geändert. Jede Linear-Order nennt die Position, auf die sie wirkt.
- **Schutz-Stops.** Ein Reduce-only-Linear-Stop geht mit Bybits *Close on Trigger* raus: er verkleinert die Position auch bei fehlender Margin, und Bybit kann dann andere Orders auf diesem Kontrakt stornieren oder verkleinern.
- **Vor der ersten Live-Order einer Sitzung** (Linear) geht die Order an Bybits Vorabprüfung, die die beanspruchte Margin nennt; eine Ablehnung dort sendet nichts. Eine bedingte erste Order wird abgelehnt: senden Sie zuerst eine einfache Market- oder Limit-Order.
- **Live-Aktualisierungen.** Orders folgen dem privaten Stream von Bybit; Positionen werden neu gelesen, wenn sie sich ändern. Ist der Stream nicht verfügbar (ein RSA-Schlüssel, eine regionale Site ohne Stream), wird das Konto in einem Zyklus gelesen.
- **Wenn die App ausfällt**, hält Bybit Ihre Orders aktiv: nichts storniert sie.
- **Take-Profit und Stop-Loss (Linear).** Take-Profit und Stop-Loss eines Tickets gehen mit dem Einstieg in einem Aufruf raus und decken die ganze Position (Bybits *Full*-Modus): jeder wird zur Market-Order, wenn sein Preis gehandelt wird. Bybit legt sie an, wenn der Einstieg ausgeführt ist; sie erscheinen im Order-Panel als Take-Profit- und Stop-Loss-Orders, mit dem Einstieg verknüpft, solange er arbeitet. Verschieben oder Entfernen ändert sie auf der Position. Ein Limit-Take-Profit wird zum Marktpreis ausgeführt, nicht als Limit.
- **Noch nicht angeboten:** Take-Profit und Stop-Loss auf Spot oder auf einen Teil der Position, OCO, Ändern einer Order an Ort und Stelle, Trailing-Stops, Post-only.

### Bybit EU

- **Nur Spot**, über denselben Orderweg wie Bybit Spot: Market- und Limit-Orders, dieselben Prüfungen vor dem Senden, eine Market-Order in der Basiswährung bemessen.
- **Testnet.** Setzen Sie die **Site** des Kontos auf `eu-testnet` (ein Schlüssel von testnet.bybit.eu): seine Orders bewegen kein Geld und brauchen keinen Live-Handel.
- **Live-Aktualisierungen.** Bybit dokumentiert für Bybit EU keinen privaten Stream: das Konto wird zyklisch gelesen.
- **Ihr Schlüssel.** Laut Bybits Leitfaden dient die API der EU-Site Drittanbieter-Anwendungen für API-Broker-Nutzer. Ein persönlicher Schlüssel kann dann abgelehnt werden: die Order zeigt Bybits Ablehnung mit ihrem Code, und nichts weiter wird gesendet.

### OKX

- **Books.** Spot, Perpetual Swaps und Futures mit Laufzeit. Jede Order geht an das Book ihres Instruments: `BTC-USDT` ist Spot, `BTC-USDT-SWAP` ein Swap, `BTC-USDT-251226` ein Future mit Laufzeit.
- **Region.** Setzen Sie die **Region** des Kontos auf die, in der es eröffnet wurde: `global` (okx.com, Standard), `eea` (my.okx.com) oder `us` (app.okx.com, USA und Australien). Ein Schlüssel funktioniert nur in seiner Region.
- **Demohandel.** Setzen Sie **Umgebung** auf `demo` mit einem Demohandel-Schlüssel: seine Orders kosten kein Geld und brauchen keinen Live-Handel.
- **Größen.** Swaps und Futures handeln in Kontrakten: eine Größe, die keine ganze Zahl von Kontrakten ist, wird mit den nächsten Größen abgelehnt.
- **Eine Order ändern**: ihren Preis und ihre Größe.
- **Der Positionsmodus** (Net oder Long/Short) wird von Ihrem Konto gelesen und nie geändert. Der Hebel wird angezeigt, nie gesetzt. Spot-Orders nutzen nur Ihre eigenen Coins, nie Margin.
- **Nicht angeboten:** Stop- und Trigger-Orders, Take-Profit und Stop-Loss, Trailing-Stops, OCO, Iceberg, Optionen, inverse Kontrakte.

### Bitget

- **Books.** Spot und USDT-M-Futures. Eine Order geht an das Book, auf das der Bitget-Datenconnector des Charts eingestellt ist (sein **Markt**: `spot` oder `usdt-futures`, auch `futures` geschrieben), sonst an das einzige Book, das das Symbol führt. Ein Symbol, das beide Books führen, auf einem Chart mit Daten von anderswo wird mit diesem Hinweis abgelehnt.
- **Demohandel.** Setzen Sie **Demo-Handel** auf `yes` mit einem Bitget-Demo-Schlüssel: seine Orders kosten kein Geld und brauchen keinen Live-Handel.
- **Stops** gehen als Bitget-Trigger-Orders, auf den letzten Preis (Futures: letzter oder Mark). Ein Spot-Stop wird zum Marktpreis ausgeführt.
- **Ein Spot-Marktkauf** ist ein Betrag in der Quote-Währung: die App liest den besten Ask kurz vor dem Senden und sendet diesen Betrag, die ausgeführte Größe kann also leicht abweichen. Die Antwort sagt es.
- **Alle Orders des Kontos** erscheinen im Order-Panel, Trigger-Orders eingeschlossen, und lassen sich dort stornieren.
- **Take-Profit und Stop-Loss einer Position (Futures).** Ein auf einer Position gesetzter erscheint im Order-Panel; ihn dort zu verschieben oder zu entfernen ändert ihn auf der Position.
- **Positionsmodus und Margin** werden von Ihrem Konto gelesen und nie geändert. Der Hebel wird angezeigt, nie gesetzt.
- **Nicht angeboten:** eine Order ändern, Take-Profit und Stop-Loss mit dem Einstieg gesendet, Trailing-Stops, Spot-Stop-Limit, OCO.

### Gate.io

- **Books.** Spot und die USDT-Perpetuals, gewählt wie bei Bitget: der **Markt** des Datenconnectors des Charts (`spot` oder `futures`, auch `usdt-futures` geschrieben), sonst das einzige Book, das das Symbol führt.
- **Testnet.** Setzen Sie die **Site** des Kontos auf `testnet` mit einem Testnet-Schlüssel: seine Orders und Abrufe gehen an das Gate.io-Testnet, kosten kein Geld und brauchen keinen Live-Handel.
- **Ein Spot-Marktkauf** ist ein Betrag in der Quote-Währung, bewertet zum besten Ask kurz vor dem Senden und auf die Genauigkeit des Paares abgerundet. Die ausgeführte Größe kann leicht abweichen; die Antwort sagt es.
- **Futures-Größen** sind ganze Kontrakte: eine andere Größe wird mit den nächsten Größen abgelehnt.
- **Eine Order ändern**: bei Spot ihren Preis oder ihre Größe (eins nach dem anderen), bei Futures ihren Preis und ihre Größe.
- **Nicht angeboten:** Stops, Take-Profit und Stop-Loss, Trailing-Stops, Post-only, OCO, Iceberg.

### KuCoin und KuCoin Futures

- **Keine Testumgebung.** KuCoin hat keine: jedes Konto ist echt und braucht Live-Handel mit der Bestätigung. Vor der ersten Order einer Sitzung schickt die App sie an den Ordertest von KuCoin, der sie prüft, ohne sie zu platzieren.
- **Spot:** Market- und Limit-Orders, in der Basiswährung bemessen.
- **Futures:** Market, Limit, Stop und Stop-Limit auf den letzten Preis, Reduce-only, Post-only. Größen sind ganze Kontrakte. Stops erscheinen im Order-Panel und lassen sich dort stornieren.
- **Positions- und Marginmodus** werden von Ihrem Konto gelesen und nie geändert. Der Hebel wird angezeigt, nie gesetzt.
- **Nicht angeboten:** eine Order ändern, Take-Profit und Stop-Loss mit dem Einstieg gesendet, Spot-Stops, OCO, coin-margined Futures, Margin-Handel.

### Kraken

- **Keine Testumgebung.** Kraken hat keine für Spot: jedes Konto ist echt und braucht Live-Handel mit der Bestätigung. Vor der ersten Order einer Sitzung schickt die App sie an den Validate-Aufruf von Kraken, der sie prüft, ohne sie zu platzieren.
- **Ihr Schlüssel.** Geben Sie ihm Query Funds, Query Open Orders & Trades, Query Closed Orders & Trades, Create & Modify Orders, Cancel/Close Orders und Access WebSocket API, nie Withdraw Funds. Geben Sie der App einen eigenen Schlüssel: Kraken lehnt einen Aufruf ab, dessen Nonce nicht über der letzten des Schlüssels liegt, ein Schlüssel, den auch ein anderes Programm nutzt, scheitert also zufällig.
- **Größen und Gebühren.** Orders werden in der Basiswährung bemessen; Gebühren werden in der Quote-Währung abgezogen.
- **Eine Order ändern** an Ort und Stelle: ihren Preis, Trigger und ihre Größe. Alle stornieren ist ein Aufruf.
- **Nicht angeboten:** Margin, eine mit dem Einstieg angehängte Close-Order, Trailing-Stops, Iceberg, Trigger auf den Indexpreis, tokenisierte Aktien.

### Kraken Futures

- **Kontrakte.** Orders gehen auf die Multi-Collateral-Kontrakte (PF_, FF_), in der Coin bemessen. Inverse Kontrakte (PI_, FI_) erscheinen mit ihren Orders und lassen sich im Order-Panel stornieren, nehmen aber keine neue Order an.
- **Demo.** Setzen Sie die **Umgebung** des Kontos auf `demo` mit einem Schlüssel aus der Kraken-Futures-Demo: seine Orders kosten kein Geld und brauchen keinen Live-Handel.
- **Ihr Schlüssel.** General API: Full Access zum Senden von Orders; ein Read-Only-Schlüssel liest weiterhin. Vergeben Sie nie die Withdrawal API.
- **Eine Order ändern** an Ort und Stelle: ihre Größe, ihr Limit und ihren Trigger.
- **Alle Orders des Kontos** erscheinen im Order-Panel, Trigger-Orders eingeschlossen, und Alle stornieren deckt sie ab.
- **Positionsmodus** ist One-Way. Der Hebel wird angezeigt, nie gesetzt.
- **Live-Aktualisierungen.** Es wird kein privater Stream genutzt: das Konto wird zyklisch gelesen. Die Gebühr eines Fills wird ohne Währung angezeigt, die Kraken nicht angibt.
- **Nicht angeboten:** Trailing-Stops, Take-Profit und Stop-Loss mit dem Einstieg gesendet, OCO, Optionen, Orders auf inversen Kontrakten.

### Coinbase

- **Kein Testkonto.** Coinbase hat keins: jedes Konto ist echt und braucht Live-Handel mit der Bestätigung. Vor der ersten Order einer Sitzung schickt die App sie an die Order-Vorschau von Coinbase.
- **Nur Spot**, in der Basiswährung bemessen. Ein Stop-Limit löst auf den letzten Preis aus.
- **Prüfungen vor dem Senden.** Tick, Größenschritt, Mindest- und Höchstgröße, aus der Produktliste von Coinbase. Ein Produkt im Limit-only- oder Post-only-Modus nimmt nur solche Orders an.
- **Eine Order ändern** an Ort und Stelle: Preis und Größe eines Limits.
- **Alle Orders des Kontos** erscheinen im Order-Panel, in der Coinbase-App platzierte Orders eingeschlossen.
- **Ihr Schlüssel.** Er braucht die Berechtigung Trade zum Senden von Orders; ein View-only-Schlüssel liest weiterhin.
- **Live-Aktualisierungen.** Orders folgen dem User-Stream von Coinbase; Fills werden im Zyklus gelesen. Ein von Coinbase angepasster Fill (Storno, Korrektur) bleibt aus dem Import und wird im Server-Log genannt; die anderen Fills bleiben erhalten.
- **Wenn die App ausfällt**, lässt Coinbase Ihre Orders weiterarbeiten.
- **Nicht angeboten:** Stop-Market, Take-Profit und Stop-Loss, Trailing-Stops, OCO, Futures und Perpetuals.

### MEXC

- **Keine Testumgebung.** MEXC hat keine: jedes Konto ist echt und braucht Live-Handel mit der Bestätigung. Vor der ersten Spot-Order einer Sitzung schickt die App sie an den Ordertest von MEXC.
- **Books.** Spot und die USDT-Perpetuals, gewählt wie bei Bitget: der **Markt** des Datenconnectors des Charts, sonst das einzige Book, das das Symbol führt.
- **Ein Spot-Marktkauf** ist ein Betrag, der in der Quote-Währung ausgegeben wird, bewertet zum besten Ask kurz vor dem Senden und auf die Genauigkeit des Paares abgerundet.
- **Futures-Größen** sind ganze Kontrakte. Neue Orders gehen nur im Hedge-Modus raus: auf einem One-Way-Konto werden sie abgelehnt, bevor etwas gesendet wird (stellen Sie das Konto bei MEXC auf Hedge-Modus um), während eine Position weiter schließt. Margin-Modus und Hebel werden gelesen, nie gesetzt.
- **Alle Orders des Kontos** erscheinen im Order-Panel, anderswo platzierte Futures-Plan-, Take-Profit-, Stop-Loss- und Trailing-Orders eingeschlossen, und Alle stornieren deckt sie ab.
- **Ihr Schlüssel.** Spot-Handel und Futures-Orderplatzierung sowie jedes Spot-Paar, das zu den Handelspaaren des Schlüssels hinzugefügt wurde; die Futures-Orderplatzierung braucht KYC.
- **Nicht angeboten:** Platzieren von Stops, Take-Profit und Stop-Loss oder Trailing-Stops, Chase-Orders, Ändern einer Order an Ort und Stelle, OCO, coin-margined Futures.

### Bitfinex

- **Kein Testhost.** Bitfinex hat keinen: jedes Konto ist echt, Paper-Subkonten eingeschlossen, und braucht Live-Handel mit der Bestätigung.
- **Books.** Die Spot-Paare der Exchange-Wallet und die Perpetuals (`BTCF0:USTF0`). Eine Order geht an das Book, auf das der Bitfinex-Datenconnector des Charts eingestellt ist (sein **Markt**: spot oder derivatives, auch perp oder futures geschrieben), sonst an das einzige Book, das das Symbol führt. Margin-Handel wird nicht angeboten.
- **Der Hebel** ist der der gehaltenen Position, nur gesendet, wenn er eine ganze Zahl von 1 bis 100 ist; ohne Position wendet Bitfinex seinen Standard (10) an. Die App wählt ihn nie.
- **Preise** tragen höchstens 5 signifikante Stellen: ein anderer Preis wird mit den zwei nächsten abgelehnt.
- **Eine Order ändern** verschiebt nur ihre Preise: um ihre Größe zu ändern, stornieren Sie sie und platzieren eine neue.
- **Ihr Schlüssel.** Schreibzugriff auf Orders zum Senden; ein Nur-Lese-Schlüssel liest weiterhin. Geben Sie der App einen eigenen Schlüssel (Bitfinex prüft eine steigende Nonce pro Schlüssel).

### Deribit

- **Books.** Futures und Perpetuals: lineare USDC-Kontrakte, in der Coin bemessen, inverse BTC- und ETH-Kontrakte, in USD bemessen. Auf einem inversen Kontrakt nimmt das Ticket die Größe in der Coin oder in USD: eine USD-Größe wird auf den Kontraktschritt abgerundet (10 USD bei BTC), eine Coin-Größe wird zum letzten Preis umgerechnet und dann abgerundet. Positionen und Orders auf inversen Kontrakten werden in USD angezeigt. Ihre Risikolimits bewerten eine in USD gegebene Order mit diesem Betrag.
- **Testumgebung.** Setzen Sie die **Umgebung** des Kontos auf `test` mit einem Schlüssel von test.deribit.com: seine Orders kosten kein Geld und brauchen keinen Live-Handel.
- **Stops** brauchen ihren Trigger (letzter, Mark oder Index): es gibt keinen Standard.
- **Post only** wird abgelehnt statt verschoben, wenn es sofort ausgeführt würde.
- **Eine Order ändern** an Ort und Stelle: ihren Preis, Trigger und ihre Größe. Ein ausgelöster Stop wird zu der Order verfolgt, die er wurde.
- **Alle Orders des Kontos** erscheinen im Order-Panel, Trigger-Orders eingeschlossen, und Alle stornieren deckt sie ab.
- **Eine Position pro Instrument.** Der Hebel wird angezeigt, nie gesetzt.
- **Nicht angeboten:** Optionen, Spot, Take-Profit und Stop-Loss mit dem Einstieg gesendet, OCO, Trailing-Stops, Iceberg.

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

Die **Schnell-Kauf/Verkauf-Leiste** (Schalter in der Werkzeugleiste) zeigt Verkaufen, eine Menge und Kaufen mit Bid und Ask. **Ein Klick sendet sofort eine Market-Order, ohne Bestätigung.** Ziehen Sie sie an ihrem Griff an eine beliebige Stelle im Chart. In der Nähe eines Rands abgelegt, rastet sie dort ein, als Zeile am oberen oder unteren Rand, als Spalte an einer Seite; anderswo abgelegt, bleibt sie, wo sie ist, und **Leiste drehen** (neben dem Zahnrad) schaltet sie zwischen Zeile und Spalte um. Ihre Position wird mit dem Arbeitsbereich gespeichert.

Ihr Zahnrad wählt eine [Einstiegsvorlage](#entry-presets) für diesen Chart: Beide Buttons senden ihre Market-Order dann mit dem Take profit und dem Stop loss der Vorlage, und mit ihrer Größe, wenn sie eine hat. Die aktive Vorlage erscheint als Chip auf der Leiste; klicken Sie darauf, um die Vorlage zu entfernen.

### Einstiegsvorlagen {#entry-presets}

Eine Vorlage ist ein Market-Einstieg mit Take profit und Stop loss in festem Abstand, bereit auf einen Klick oder eine Taste. Öffnen Sie die Liste über das Zahnrad der Schnellleiste (**Vorlagen bearbeiten…**) oder über die Vorlagenleiste des Tickets. Die Vorlagen gehören zu den Chart-Einstellungen: Jeder Arbeitsbereich bietet dieselbe Liste.

| Feld | |
|---|---|
| **Name** | wird auf den Chips und in den Menüs angezeigt |
| **Richtung** | Long, Short oder **Long / Short**: Ticket und Tasten behalten dann die Seite, die das Ticket zeigt. In der Schnellleiste ist die Richtung immer die des Buttons |
| **Größe** | eine Größe in Einheiten oder **Die Größe des Charts** (die, die die Leiste oder das Ticket zeigt) |
| **Take Profit**, **Stop Loss** | ein Abstand vom Einstiegspreis oder **Aus**. Einheiten: **Ticks** des Instruments, **Punkte** des Preises, ein **Betrag** in Geld auf die gesamte Position oder ein **%** des Preises |
| **Taste** | 1 bis 9 oder keine. Eine Taste, die schon eine andere Vorlage belegt, wird zu dieser verschoben |

- **Die Ausstiege werden ab der Seite berechnet, die der Einstieg kreuzt**: dem Ask bei einem Kauf, dem Bid bei einem Verkauf (dem letzten Preis, wenn es keinen Kurs gibt), dann auf den Tick des Instruments gerundet.
- **Schnellleiste**: Ein Klick sendet den Market-Einstieg und seine Ausstiege sofort, ohne Bestätigung, wie die einfachen Orders der Leiste.
- **Ticket**: Die Vorlagenleiste über der Seite füllt das Ticket mit einer Vorlage (Seite, Market, Größe, Preise für Take profit und Stop loss). Nichts geht raus, bis Sie es senden.
- **Tasten 1 bis 9**: aus, bis Sie im Vorlagendialog **Tasten 1 bis 9 füllen das Orderticket** ankreuzen. Eine Taste öffnet das Ticket, gefüllt mit ihrer Vorlage; Sie senden es von dort. Tasten bewirken nichts, während Sie in ein Feld tippen oder solange ein Dialog offen ist.
- Eine Vorlage, die sich nicht berechnen lässt, wird mit der Abhilfe abgelehnt: noch kein Preis (warten Sie auf einen), ein Abstand in Ticks bei einem Instrument mit unbekannter Tickgröße (setzen Sie sie in den Instrumenteinstellungen oder nutzen Sie Punkte), ein Betrag ohne Größe oder ein Stop loss, der bei oder unter null landen würde.

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
| **Verlustlimit pro Zeitraum** | alles, sobald das Konto seit Beginn des Zeitraums (Stunde, Tag oder Woche, siehe unten) so viel verloren hat: das Konto wird angehalten. Seine Einheit ist ein **Betrag** in der Kontowährung oder ein **% des Kapitals**, mit dem der Zeitraum begann |
| **Duplikatfenster (Sekunden)** | dieselbe Order (Seite, Typ, Größe, Preise), die innerhalb dieser Sekunden erneut gesendet wird, bis Sie bestätigen |

- Eine Verletzung wird mit Limit und Wert abgelehnt. **Die Order wird nie verkleinert, damit sie passt.**
- **Risiko abbauen geht immer durch**: eine Reduce-only-Order, ein Schließen oder eine Order, die nur das Gehaltene verkleinert, passiert jedes Limit und jeden Halt.
- Ein Limit zu ändern verlangt Ihr Passwort.

### Halts

Ein Halt lehnt neue Orders ab, bis er endet; Stornierungen und Orders, die eine Position verkleinern, gehen weiterhin raus, und aktive Orders lassen sich in der Zwischenzeit nicht ändern (stornieren Sie sie stattdessen).

- **Notschalter**: Er storniert alle aktiven Orders auf **jedem** Trading-Konto (anderswo platzierte Orders eingeschlossen, jede Stornierung im Audit-Log) und hält dann alle an. Unter **Einstellungen → Broker** verlangt er eine Bestätigung. Im Chart sitzt er in der Werkzeugleiste neben **Einfrieren**, solange ein Konto handelt, und funktioniert genauso: Ein Klick macht ihn für 3 Sekunden scharf, ein zweiter Klick löst ihn aus. Nie ein Passwort.
- **Einfrieren** (Werkzeugleiste des Charts): sperrt 10 Minuten lang jeden neuen Einstieg, der aus den Charts gesendet wird, auf jedem verbundenen Konto (Paper eingeschlossen), und storniert aktive Einstiegsorders. Schließen, Verkleinern, Stops und Ziele funktionieren weiter. Ein Klick macht es scharf, ein zweiter startet es; nichts beendet es vorzeitig.
- **Verlustlimit**: Ein Konto, das sein Verlustlimit erreicht, hält sich selbst an (geprüft vor jeder Order und im Abgleichzyklus), und Sie erhalten eine Benachrichtigung.
- **Positionsabweichung**: Bewegt sich eine Position ohne Fills, die das erklären (eine Order, die die App nicht gesehen hat, ein korrigierter Fill), hält sich das Konto an, bis Sie es **bestätigen**, und Sie erhalten eine Benachrichtigung. Prüfen Sie zuerst das Konto beim Broker; das Bestätigen verlangt immer Ihr Passwort, unabhängig von der Richtlinie unten.

Unter **Einstellungen → Broker → Risiko und Notschalter** wählen Sie für alle Konten, wie ein Halt endet:

- **nach**: Beginn der nächsten Stunde, des nächsten Tages oder der nächsten Woche (Montag) in der **Zeitzone** der Regel; oder **nur wenn aufgehoben**. Die Zone ist UTC, bis Sie eine speichern, und das Formular startet mit der Zone Ihres Browsers;
- **früher mit meinem Passwort aufheben**: Ein Halt kann vorzeitig am selben Ort oder über das Banner des Charts aufgehoben werden. Ein Halt, der nur durch Aufheben endet, wird immer mit dem Passwort aufgehoben.

Das Speichern der Regel verlangt Ihr Passwort.

Der Verlustzeitraum folgt derselben Wahl (ein Tag, wenn Halts nur durch Aufheben enden). Der Verlust wird ab dem Eigenkapital des Kontos zu Beginn des Zeitraums gemessen, wie es der Broker angibt; eine Einzahlung oder Auszahlung im Zeitraum zählt mit. Ein Broker, der kein Eigenkapital angibt (Binance Spot Demo Mode), kann an kein Verlustlimit gebunden werden: Seine Orders werden abgelehnt, solange eines gesetzt ist.

## Wächter für den Chart-Handel {#guards}

Wächter überwachen die Trades, die Sie während einer **Sitzung** aus den Charts senden, und handeln, wenn eine Regel erreicht ist. Öffnen Sie sie über **Wächter** in der Werkzeugleiste des Charts: Der Button zählt die eingeschalteten Wächter und wird bernsteinfarben, solange ein Wächter eine Sitzung gestoppt hält.

- **Nur der Handel aus den Charts zählt**: Orders, die aus einem Chart gesendet werden (Ticket, Schnellleiste, das `+` der Preisskala), und ihre Ausführungen. Roboter und Trades bei Alarm werden weder gezählt noch gestoppt.
- Ein Wächter deckt ein Brokerkonto oder **Jedes Konto** ab, jedes Konto für sich gelesen. Angeboten werden die Konten, denen unter **Einstellungen → Broker** Trading und die Charts freigegeben sind. Höchstens 30 Wächter.
- Die Regeln werden alle 20 Sekunden auf dem Server gelesen und erneut direkt nach einer Ausführung im Chart, sie arbeiten also auch ohne offenen Chart weiter.
- **Jedes Ereignis löst einmal aus**: Eine erneut gelesene Regel findet vor, was sie bereits getan hat.

### Sitzungen

**Eine Sitzung endet** **Jeden Tag (am nächsten Tag)**, um Mitternacht in der gewählten Zeitzone (das Formular startet mit der Zone Ihres Browsers), oder **Nur wenn ich sie zurücksetze**. **Sitzung zurücksetzen** (ein Konto) oder **Alle zurücksetzen** im Dialog startet immer eine neue Sitzung, löscht, was ausgelöst wurde, und hebt einen Stopp auf.

### Regeln

| Regel | Löst aus, wenn |
|---|---|
| **Verlusttrades** | N Verlusttrades, **In Folge** oder **Über die Sitzung** (erneut bei jedem weiteren N) |
| **Anzahl der Trades** | N Einstiege innerhalb eines Zeitfensters (Minuten oder Stunden): **Gleitend** oder **Jeder Zeitraum ab Sitzungsbeginn** |
| **Drawdown seit Sitzungsbeginn** | das Equity des Kontos um einen Betrag oder einen Prozentsatz unter seinen Wert bei Sitzungsbeginn gefallen ist |
| **Verlust bei einem Trade** | ein Trade mehr als einen Betrag verloren hat, oder mehr als einen Prozentsatz vom **Wert des Trades** oder vom **Equity bei Sitzungsbeginn** |
| **Gebühren** | die Gebühren der Sitzung einen Betrag in der Kontowährung erreichen |

- **Ein Trade** ist ein Round Trip, rekonstruiert aus den Chart-Ausführungen der Sitzung, pro Instrument, zu Durchschnittskosten: Er endet, wenn das Instrument wieder flat ist, und sein Ergebnis versteht sich nach Gebühren. Eine vor der Sitzung gehaltene Position hat keine bekannten Kosten: Der Round Trip, der sie schließt, zählt nicht.
- **Equity** ist die eigene Kennzahl des Brokers, gelesen bei der ersten Prüfung der Sitzung; eine Einzahlung oder Auszahlung zählt mit. Ein Broker, der kein Equity angibt, kann weder die Drawdown-Regel ausführen noch einen Verlust in Prozent des Equity.
- **Gebühren**, die in der Währung des Instruments oder im gekauften Coin berechnet werden, zählen mit; andere nicht.
- Ein Broker, der keine aktuellen Ausführungen liefert, gibt den Regeln zu Trades und Gebühren nichts zu lesen: Sie lösen dort nie aus.

### Was passiert

| Aktion | |
|---|---|
| **Alarm** | ein Hinweis auf dem Chart |
| **Fenster zur Bestätigung** | ein Fenster über dem Chart, das Sie verschieben können, bis Sie auf **Bestätigen** (oder **Alle bestätigen**) klicken. Nichts wird blockiert |
| **Sitzung stoppen** | neue Chart-Einstiege auf dem Konto werden abgelehnt, bis Sie die Sitzung zurücksetzen (oder, bei täglichen Sitzungen, bis zum nächsten Tag). Orders, die eine Position verkleinern, gehen weiterhin raus. Der Chart zeigt den Stopp in seinem Halt-Banner, mit einem Button zum Zurücksetzen. Option: **Auch die offenen Orders des Charts stornieren und die von ihm eröffneten Positionen schließen**, zum Marktpreis, auf den Instrumenten, die die Sitzung gehandelt hat |
| **Not-Aus** | der [Notschalter](#halts), so wie sein Button ihn auslöst: alle aktiven Orders storniert, alle Trading-Konten angehalten |

Jede Auslösung und jedes Zurücksetzen wird ins Sicherheitsprotokoll geschrieben, und die Orders, die ein Stopp storniert oder schließt, gehen wie die eigenen Orders des Charts ins [Audit-Log](#audit-log).

## Live-Handel {#live-trading}

Die Orderstrecke jedes Brokers hat eine Stufe, die am Konto unter **Einstellungen → Broker** angezeigt wird: E2 (gegen einen simulierten Handelsplatz getestet), E3 (über Tage in der eigenen Testumgebung des Brokers getestet), E4 (ein Live-Probelauf und eine Live-Order in kleinster Größe sind bestanden). Unterhalb von E4 wird ein Konto, das echtes Geld handelt, vom Connector selbst abgelehnt.

| Stufe | Bedeutet | Live-Orders |
|---|---|---|
| **E2** | gegen eine simulierte Plattform getestet | abgelehnt |
| **E2D** | E2, allein nach der Dokumentation der Plattform gebaut, nie gegen sie getestet | auf einem Konto, dessen Inhaber das bestätigt hat, mit gesetzten Limits; die eigene Prüfung der Plattform läuft vor der ersten Order jeder Sitzung, wo es eine gibt |
| **E3** | über Tage in der Testumgebung des Brokers getestet | abgelehnt |
| **E4** | ein Live-Probelauf und eine Live-Order in kleinster Größe bestanden | sobald Live-Handel eingeschaltet ist, mit gesetzten Limits |

**E2D (Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex, Deribit).** Das Einschalten des Live-Handels verlangt ein Häkchen bei der Bestätigung, dass der Orderweg dieses Brokers nach der Dokumentation der Plattform gebaut und nie gegen sie getestet wurde; sie wird mit Datum gespeichert und auf dem Konto angezeigt. Es gelten dieselben Limits wie bei E4. Eine Antwort, die der Broker nicht versteht, eine Order mit unbekanntem Ausgang oder eine Position, die sich ohne Ausführungen bewegt, wird wie bei jedem Broker behandelt: protokolliert, gemeldet und mit dem Broker abgeglichen.

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

## Handel bei Alarm {#trade-on-alert}

Ein Chart-Alarm kann beim Auslösen eine Order senden, ohne dass jemand zusieht, auch bei geschlossener App. Richten Sie das im Abschnitt **Trade** des Alarms ein (*Beim Auslösen dieses Alarms eine Order an ein Broker-Konto senden*); der Alarm selbst ist unter [Charts → Alarme](/de/modules/market-data#alerts) beschrieben.

- **Das Konto** muss Trading eingeschaltet haben, für Charts freigegeben sein und [Hintergrundhandel](/de/config/brokers#background-trading) tragen, einmal unter **Einstellungen → Broker** unterzeichnet. Ein Live-Konto braucht außerdem eingeschalteten [Live-Handel](#live-trading) mit gesetzten Limits.
- **Beim Auslösen**: **Einstieg** sendet das Ticket, das Sie beschreiben (Seite, Typ, Größe in **Einheiten**, **Betrag** oder **Risiko**, Preise als Abstand zum auslösenden Preis, Take-Profit und Stop-Loss); **Ausstieg** storniert die aktiven Orders auf dem Instrument und schließt seine Position zum Marktpreis.
- **Bei Ablehnung**: nichts weiter tun, die Orders auf dem Instrument stornieren, es zusätzlich schließen oder alles stornieren und jede Position schließen. Geht die Antwort des Brokers verloren, werden nur die Orders auf dem Instrument storniert.
- **Vorschau** zeigt die Order zum Preis des Charts, den Probelauf des Brokers, wo er einen hat, und was sonst auf dem Instrument scharf geschaltet ist. **Scharf schalten** folgt auf eine Vorschau und fragt nach Ihrem Authenticator-Code, wenn die [Trading-Sperre](#trading-lock) aktiv ist und das Konto kein Papierkonto ist.
- **Er löst einmal aus** und wartet dann darauf, erneut scharf geschaltet zu werden. Jede Order durchläuft jede Prüfung, die eine Chart-Order durchläuft ([Risikolimits](#risk-limits), Halts, das Audit-Log, mit dem Alarm als Quelle), und das Ergebnis wird über die Kanäle des Alarms gemeldet.
- **Entschärfen.** Das Ausschalten von Hintergrundhandel oder Trading auf dem Konto, das Entfernen der Zwei-Faktor-Authentifizierung, das Einschalten der Trading-Sperre oder der Notschalter entschärfen, was scharf geschaltet war; schalten Sie es bewusst erneut scharf.

## Grenzen

- Bei Interactive Brokers gehen Orders an das Standardkonto des Gateways.
- Keine Änderung von Seite oder Typ einer aktiven Order: stornieren und neu senden.
- Orders lauten auf die Währung des Kontrakts, ohne Umrechnung.
- KI-Agenten ([MCP](/de/config/ai-agents)) und Automator-Workflows können keine Orders senden und keinen Handel bei Alarm scharf schalten. Ein [Trading-Bot](/de/modules/robots) oder ein [Handel bei Alarm](#trade-on-alert) sendet Orders erst, wenn Sie ihn scharf schalten, auf einem Konto mit unterzeichnetem [Hintergrundhandel](/de/config/brokers#background-trading).
- Broker-Konten, und damit das Trading, sind im [Demo-Modus](/de/guide/demo) deaktiviert.
