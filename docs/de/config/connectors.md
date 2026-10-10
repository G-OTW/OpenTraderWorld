# Daten-Konnektoren

Jedes Modul, das Marktdaten liest, bezieht sie aus **einer gemeinsamen Liste von Konnektoren**. Ein Anbieterkonto wird einmal angelegt und den Modulen gewährt, die es nutzen dürfen.

Verwalte sie unter **Einstellungen → Daten-Konnektoren**, auf der eigenständigen Seite **/connectors** oder über die Konnektor-Schaltfläche, die jedes Datenmodul neben seine Anbieterauswahl setzt. Alle drei zeigen dieselbe Ansicht.

## Was ein Konnektor ist

Ein **Konnektor ist eine benannte Instanz eines Anbieters**, nicht der Anbieter selbst. Vier Dinge gehören dazu:

- **der Anbieter**: Binance, Yahoo Finance, EODHD…;
- **seine Zugangsdaten**: eingetippt oder aus dem [Tresor](/de/config/settings#vault) eingesteckt. Nur schreibbar: Die App kennt immer nur, *welche* Geheimnisnamen gesetzt sind;
- **ein optionales Anfragelimit**: eine maximale Anzahl Aufrufe pro Zeitraum;
- **die Module, die ihn nutzen dürfen**: eines oder mehrere, oder *alle Module* (ein Platzhalter, der auch Datenmodule künftiger Versionen abdeckt).

Mehrere Konnektoren desselben Anbieters können nebeneinander existieren. Das ist der Sinn: ein schreibgeschützter Schlüssel für das Charting und ein separater Schlüssel für Massen-Downloads, jeder mit eigenem Limit, jeder einem anderen Modul gewährt.

## Anbieter

| Anbieter | Zugangsdaten | Anlageklassen | Symbolsuche | Live-Stream |
|---|---|---|---|---|
| **Binance** | keine | Krypto | ja | ja |
| **Binance USDⓈ-M Futures** | keine | Krypto | ja | ja |
| **Bitget** | keine | Krypto | ja | ja |
| **OKX** | keine | Krypto | ja | ja |
| **Kraken** | keine | Krypto | ja | ja |
| **Coinbase** | keine | Krypto | ja | ja |
| **OANDA** | `api_token` (+ Konto-ID) | FX und CFDs | ja | nein |
| **Yahoo Finance** | keine | Aktien, ETF, Index, Krypto | ja | nein |
| **Alpha Vantage** | `api_key` | Aktien, ETF, Krypto, FX | ja | nein |
| **EODHD** | `api_key` | Aktien, ETF, FX, Krypto | ja | nein |
| **Alpaca** | `api_key`, `api_secret` | Aktien, Krypto, Optionen | ja | Intraday |
| **Massive (Polygon.io)** | `api_key` | Aktien, ETF, Optionen, Futures, Krypto, FX, Index | ja | Intraday, kostenpflichtiger Tarif |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` | Aktien, ETF, Optionen, Futures, Index | per Symbol | nein |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | FX und CFDs | ja | nein |
| **Capital.com** | `api_key`, `identifier`, `api_password` | FX, Indizes, Aktien, Krypto (alle CFDs) | ja | ja |
| **Interactive Brokers** | keine (Host + Port) | Aktien, ETF, Krypto, FX, Index, Futures, Optionen | ja | Intraday |

Die ohne Schlüssel funktionieren, sobald du den Konnektor anlegst. Jede Anbieterzeile verlinkt auf die eigene API-Dokumentation und trägt einen Hinweis zu den Rate-Limits, und ein Konnektor mit Streaming trägt außerdem einen Hinweis, was Live dort kostet.

### Live-Streaming {#live-streaming}

Die Live-Reichweite ist enger als die Download-Reichweite, und zwar bewusst.

- Die Krypto-Börsen veröffentlichen einen Candle-Kanal pro Intervall, daher streamen sie **jeden** Timeframe, den sie auch liefern, auch den täglichen: In einem 24/7-Markt *ist* die Tageskerze der Epochentag. Bitget und OKX verankern ihre eigenen Tages- und Wochenkerzen auf Mitternacht in Hongkong, daher fragen sowohl der Download als auch der Live-Feed deren UTC-ausgerichtete Varianten an, und eine dort geladene Serie passt zu einer, die anderswo geladen wurde.
- **Drei Anbieter streamen hier nicht.** OANDA und TradeStation veröffentlichen Live-Kurse über eine langlebige HTTP-Antwort und FOREX.com über Lightstreamer; keiner der drei ist der WebSocket, den die Live-Charts sprechen. Ihre Verlaufs-Downloads und ihre Kontoseite funktionieren; die Live-Kerze nicht.
- Alpaca, Massive und Interactive Brokers veröffentlichen je eine Granularität (Ein-Minuten-Bars, Ein-Minuten-Aggregate bzw. Fünf-Sekunden-Bars), und der Timeframe des Charts wird daraus gebildet. Das macht jeden **Intraday**-Timeframe verfügbar und überlässt **täglich und wöchentlich dem Download**: Eine Aktiensitzung sind nicht 1440 epochenausgerichtete Minuten, eine so gebildete Tageskerze würde also von der abweichen, die der Download speichert. Der Chart sagt das, statt das Bedienelement zu verstecken.
- Live wird oft getrennt vom Verlauf verkauft. Ein kostenloser Massive-Schlüssel lädt den Verlauf und wird beim Live-Login abgelehnt; der kostenlose Schlüssel von Alpaca streamt IEX und den indikativen Optionsfeed, aber nicht SIP oder OPRA; Interactive Brokers liefert, was dein Konto abonniert hat. Kann ein Feed nicht laufen, nennt der Chart, welcher dieser Fälle es ist, und hält an, statt sich hinter einem Punkt, der nie grün wird, immer wieder zu verbinden.
- Die meisten dieser Anbieter erlauben **eine Live-Verbindung pro Konto**, ein zweites Programm mit demselben Schlüssel nimmt also den Platz. Dieser Fall wird als solcher gemeldet und versucht es weiter, da er sich löst, wenn du das andere schließt.

**Alpaca** trägt dafür eine Einstellung: *Marktdaten-Feed*, `iex` (kostenloser Tarif, der Standard) oder `sip` (kostenpflichtig). Sie wählt nur den Live-Socket; Downloads bleiben unberührt.

### Die Krypto-Derivate-Anbieter

`BTCUSDT` ist ein Spot-Paar **und** ein Perpetual, und die beiden sind verschiedene Serien: Der Perpetual handelt mit einer Basis zum Spot, und ein Kontrakt mit Fälligkeit konvergiert dorthin. Welchen Markt ein Datensatz enthält, wird daher nie aus dem Ticker abgeleitet.

- **Binance USDⓈ-M Futures** ist ein eigener Anbieter neben Binance, keine Einstellung daran. Kontrakte werden so geschrieben, wie der Futures-Markt sie schreibt: `BTCUSDT` für einen Perpetual, `ETHUSDT_250926` für einen mit Fälligkeit. Coin-M-Kontrakte (inverse) werden nicht bedient.
- **Bitget** trägt eine Einstellung *Markt*, `spot` (der Standard) oder `usdt-futures`, weil es denselben Ticker in beiden Büchern identisch schreibt.
- **OKX** braucht keine Einstellung: Seine eigenen Instrument-IDs sagen, in welchem Markt ein Ticker liegt, `BTC-USDT` für Spot, `BTC-USDT-SWAP` für einen Perpetual, `BTC-USD-241227` für einen Kontrakt mit Fälligkeit.

### OANDA

Der einzige FX-Anbieter hier mit Schlüssel, und sein Schlüssel ist der des Kontos: OANDA stellt kein reines Marktdaten-Token aus, daher fragt der Konnektor nach demselben persönlichen Zugriffstoken, das das [Broker-Konto](/de/config/brokers) nutzt, plus der Kontonummer, über die er Kurse liest.

- **Einstellungen**: *Konto-ID* (`001-004-1234567-001`) und *Umgebung* (`live` oder `practice`, die verschiedene Hosts mit verschiedenen Tokens sind).
- **Instrumente** werden als `base_quote` geschrieben, auch Indizes und Rohstoffe: `EUR_USD`, `XAU_USD`, `SPX500_USD`. *Verbindung testen* meldet, wie viele das Konto bepreisen darf.
- **Tageskerzen sind auf Mitternacht UTC festgelegt.** Der eigene Standard von OANDA wechselt den Tag um 17:00 New York, was die FX-Sitzung ist, aber nicht der Tag, auf dem jeder andere Datensatz hier gespeichert wird, daher fragt der Konnektor den UTC-Tag an.
- Das Volumen ist eine **Tick-Anzahl**, keine gehandelte Größe: Ein Dealing Desk veröffentlicht, wie viele Kurse er gestellt hat, nicht wie viel den Besitzer gewechselt hat.

Hält ein Anbieter mehrere Konnektoren, bekommt das Live-Bedienelement des Charts eine Kontoauswahl: Zwei Schlüssel sind zwei Berechtigungen und zwei Verbindungsplätze, welcher davon verbraucht wird, ist also deine Wahl, kein Fallback.

### TradeStation

Sein Marktdaten-Scope hängt am OAuth-Schlüssel des Kontos, daher fragt der Konnektor nach demselben API-Schlüsselpaar und Refresh-Token, das das [Broker-Konto](/de/config/brokers) nutzt. Es gibt keine separaten Marktdaten-Zugangsdaten.

- **Einstellungen**: *Umgebung* (`live` oder `sim`).
- **Symbole** sind die von TradeStation: `AAPL` für eine Aktie, `@ES` für den kontinuierlichen Future, `ESH26` für einen Kontrakt, `$SPX.X` für einen Cash-Index, `MSFT 260116C400` für eine Option. Beim Eintippen wird eines nachgeschlagen und angezeigt, was es ist, was der schnellste Weg ist, einen Tippfehler zu finden.
- **Bars werden zu ihrem Schluss gestempelt**, daher zieht der Konnektor das Intervall ab und speichert die Eröffnung, wie jede andere Serie hier.
- **Nur 1m, 5m, 15m und 1d werden angeboten.** TradeStation baut Intraday-Bars ab der *Sitzungs*-Eröffnung, seine Stundenkerze beginnt also um 9:30 und würde nicht zur Stundenkerze irgendwo sonst in deiner Bibliothek passen. Lade stattdessen 15m und lies es in jedem Intraday-Timeframe; die Ablehnung sagt das.

### FOREX.com (StoneX)

Gleiche Geschichte: keine eigenen Marktdaten-Zugangsdaten, daher meldet er sich mit demselben Benutzernamen, Passwort und AppKey an wie das [Broker-Konto](/de/config/brokers), und beide teilen sich eine Sitzung.

- **Ein Markt ist eine Zahl.** Die API nimmt eine numerische Markt-ID; du tippst `EUR/USD`, und der Konnektor löst es auf. Passt ein Name auf mehrere Märkte, listet der Fehler sie mit ihren IDs auf, und du lädst per ID.
- **Kein Volumen.** Ein Dealing Desk veröffentlicht Kurse, keine Größe, daher ist die Volumenspalte null statt einer plausiblen Zahl.
- **Ein Tagesbar ist die Sitzung des Handelsplatzes**, die beim Schluss in New York wechselt, nicht um Mitternacht UTC. Das ist der Zeitraum, in dem StoneX tatsächlich gehandelt hat, und er wird so gespeichert, sodass eine Tagesserie von hier nicht deckungsgleich mit einer von einem UTC-Tag-Anbieter liegt.
- `4h` wird abgelehnt: StoneX sagt nicht, wo es zu zählen beginnt. Lade `1h` und lies es als 4h.

### Capital.com

Der dritte mit Schlüssel, dessen Schlüssel der des Kontos ist: Capital.com stellt keine Marktdaten-Zugangsdaten aus, daher meldet sich der Konnektor mit demselben API-Schlüssel, Login und benutzerdefinierten Passwort an wie das [Broker-Konto](/de/config/brokers), und beide teilen sich eine Sitzung.

- **Einstellungen**: *Umgebung* (`live` oder `demo`).
- **Ein Instrument ist ein Epic**, der eigene Marktname von Capital.com: `EURUSD`, `US500`, `AAPL`, `BTCUSD`. Die Symbolsuche liefert sie.
- **Eine Kerze ist die Mitte der beiden Seiten**, auf denen der Desk handelt, im Download und im Live-Chart gleichermaßen.
- **Ein Tagesbar ist die Sitzung des Handelsplatzes**, nicht der UTC-Tag, sodass eine Tagesserie von hier nicht deckungsgleich mit einer von einem UTC-Tag-Anbieter liegt.
- **Live** nutzt dieselbe Sitzung und erlaubt 40 Instrumente gleichzeitig. Capital.com streamt Bid und Ask als zwei getrennte Kerzen, ein Pane füllt sich also erst, wenn beide Seiten getickt haben.

### Interactive Brokers {#interactive-brokers}

Der Außenseiter: Es gibt keine Anbieter-URL und keinen API-Schlüssel. Du betreibst **IB Gateway** oder **TWS** auf deiner eigenen Maschine, und der Konnektor spricht dessen Socket-Protokoll, was er trägt, ist also eine **Adresse**, keine Zugangsdaten: ein Host und ein Port, im Klartext gespeichert, damit sich eine fehlgeschlagene Verbindung diagnostizieren lässt. Die Daten sind das, was dein IB-Konto abonniert hat.

- **Einstellungen**: *Gateway-Host* (`host.docker.internal` für ein Gateway auf derselben Maschine, da OpenTraderWorld in einem Container läuft) und *API-Port* (4001 live / 4002 paper für das Gateway, 7496 / 7497 für TWS).
- **Im Gateway**: Global Configuration → API → Settings, setze den Haken bei *Enable ActiveX and Socket Clients* und prüfe, dass der Port passt. Unter Docker Desktop kommt der Aufruf vom Host-Loopback, daher deckt *Allow connections from localhost only* ihn bereits ab; unter Docker Engine entferne den Haken und füge `172.28.53.10` zu *Trusted IPs* hinzu, das einzelne Adressen nimmt und keinen Bereich.
- **Verbindung testen** meldet, was geantwortet hat, und nennt die zu ändernde Einstellung, wenn nichts antwortet.
- **Ticker**: `AAPL`, `SAN:EUR` oder `7203@TSEJ:JPY` für Aktien, `EURUSD` für ein Cash-Paar, ein OCC-Symbol für eine Option. Ein Future wird mit seinem Monat geschrieben, `ES.202512`, oder mit dem lokalen Symbol, das TWS zeigt, `MNQU6`. Futures werden vor jedem Download am Gateway nachgeschlagen, die Börse ist also optional: Nennt der Ticker mehr als eine Notierung, listet der Fehler sie auf, und du wählst.
- Die Client-ID wählt die App in einem hohen privaten Bereich, sie wird nie abgefragt, sodass nichts anderes, was du mit dem Gateway verbunden hast, hinausgeworfen wird.
- Interactive Brokers erlaubt 60 historische Anfragen pro gleitenden 10 Minuten **pro Konto**: Der Konnektor taktet sich selbst, ein langer Backfill ist also absichtlich langsam.

Getestet mit **IB Gateway Build 10.50.1e (25. Aug. 2026)**. Ältere Builds sollten funktionieren, da das Socket-Protokoll nach unten ausgehandelt wird, aber diese ist die Version, mit der dieser Konnektor verifiziert wurde.

## Einen anlegen

1. **Konnektor hinzufügen**, wähle den Anbieter und benenne ihn: Der Name ist, was die Modulauswahlen zeigen, *Binance charts* ist also besser als *Binance 2*.
2. Fülle die vom Anbieter verlangten Zugangsdaten aus oder wähle sie aus dem Tresor. Anbieter ohne Schlüssel überspringen das.
3. Wähle die **Module**, die er bedient. Aus einem Modul geöffnet, wird der neue Konnektor nur diesem Modul gewährt; aus den Einstellungen oder `/connectors` geöffnet, wird er allen gewährt.
4. Setze optional ein **Anfragelimit** (siehe unten).

Ein Konnektor, dem eine erforderliche Zugangsangabe fehlt, wird als *braucht Zugangsdaten* angezeigt und von jedem Modul übersprungen, bis du sie setzt.

## Modulfreigaben

Die Freigabeliste ist **serverseitig**: Ein Modul, das nach einem Konnektor fragt, der ihm nie gegeben wurde, wird abgelehnt, ein Häkchen, das nur im Browser lebte, wäre also Dekoration gewesen. Werden alle Datenmodule angekreuzt, fällt es zurück auf den Platzhalter *alle Module*, was künftige Datenmodule abgedeckt hält.

Die heute freigebbaren Module:

| Modul | Was es liest |
|---|---|
| **Historical Data** | die Anbieterliste des Download-Formulars und die Symbolsuche |
| **Visualization** | die Symbolsuche des Charts, seine Abrufe auf Anforderung und seinen Live-Stream |
| **Watchlists** | die Kursquelle einer Liste oder eines einzelnen Symbols |
| **Journal** | die Kerzen hinter den Tabs Marktdaten und Offenes Risiko |
| **Quant Tools** | die Derivate-Tabs: Futures-Kontrakte, Optionsketten und implizite Volatilität, von Interactive Brokers oder Massive |

## Anfragelimits {#request-limits}

Ein Limit ist eine Zahl ausgehender Aufrufe pro **Tag**, **Stunde** oder **Minute**, pro Konnektor verfolgt.

- Überall sonst in der App ist es **nur beobachtend**: Es speist die Zähler unter [Einstellungen → API-Rate](/de/config/settings#api-rate) und warnt dich, aber nichts wird gedrosselt.
- Bei den Abrufen auf Anforderung im Chart (`/api/histviz/series`) **blockiert** es: Sobald der Konnektor an seinem Limit ist, kommt das Fenster mit den bereits gespeicherten Bars und einem Hinweis *Anfragelimit erreicht* zurück, statt stillschweigend einen kontingentierten Tarif aufzubrauchen.

Lass das Limit weg, wenn du lieber den Anbieter derjenige sein lässt, der Nein sagt.

## Wo Konnektoren genutzt werden

- **Historical Data**: die Anbieterliste des Download-Formulars und die Symbolsuche.
- **Visualization**: Der Tab Daten durchsucht alle dem Chart gewährten Konnektoren auf einmal; der Live-Stream läuft über den Konnektor, den du im Live-Bedienelement wählst, oder über den ältesten, der dem Chart für diesen Anbieter gewährt ist.
- **Watchlists**: die Kursquelle einer Liste oder eines einzelnen Symbols. CoinGecko und Yahoo bleiben ganz ohne Konnektor verfügbar.
- **Trading Journal**: die Kerzen hinter den Tabs Marktdaten und Offenes Risiko, mit einer Quelle, die pro Anlageklasse wählbar ist.
- **Quant Tools**: Die Derivate-Tabs listen die Futures-Kontrakte eines Produkts oder die Optionskette eines Basiswerts auf und bepreisen jeden. Nur Interactive Brokers und Massive listen sie; die Historie der impliziten Volatilität kommt allein von Interactive Brokers.

::: tip Upgrade von den alten Einstellungen pro Modul
Der Tab *Einstellungen* von Historical Data und der Tab *Quellen* der Watchlists existieren nicht mehr: Sie waren zwei unverbundene Kopien dieser Ansicht über zwei unverbundene Listen. In einem von beiden angelegte Konten sind jetzt Konnektoren hier, jeweils weiterhin dem Modul gewährt, aus dem sie stammen, sodass sich beim Upgrade an der Reichweite nichts ändert. Namen sind wieder global eindeutig: Ein Name, der in beiden Listen existierte, bleibt einmal erhalten, und der andere wird in `<name> #2` umbenannt.
:::
