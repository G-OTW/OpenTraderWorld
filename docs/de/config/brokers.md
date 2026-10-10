# Broker-Konten

Ein **Broker-Konto** ist ein schreibgeschützter Schlüssel zu dem Ort, an dem du tatsächlich handelst. Es beantwortet drei Fragen, die niemand von Hand neu eintippen sollte: Was wurde ausgeführt, was halte ich, was liegt im Markt.

Verwalte sie unter **Einstellungen → Broker** oder über die Schaltfläche *Broker*, die jedes Modul, das dein Buch liest, neben seine Kontoauswahl setzt. Beide zeigen dieselbe Ansicht.

::: warning Nur lesend, konstruktionsbedingt
Broker-Konten lesen und nur lesen: Keine Route dahinter gibt eine Order auf, ändert oder storniert sie. Die verlangten Zugangsdaten sind die schreibgeschützte Art, gib also genau diese an: Wo ein Broker einen reinen Ansichts-Schlüssel ausstellen kann, sagt das Formular es. Sollte Order-Routing je erscheinen, wird es eine eigene Funktion sein, die eigene Schlüssel und eine eigene Berechtigung verlangt, und es wird hier stehen.
:::

Nicht zu verwechseln mit [Daten-Konnektoren](/de/config/connectors): Diese lesen **Kurse**, jene lesen **dein Konto**. Zwei verschiedene Zugangsdaten, zwei verschiedene Listen, zwei verschiedene Freigaben, mit Absicht.

## Was du zum Verbinden brauchst

| Broker | Zugangsdaten | Zu erteilende Berechtigung | Liest |
|---|---|---|---|
| **Alpaca** | `api_key`, `api_secret` | Trading-API-Schlüssel der gewählten Umgebung (live *oder* paper, es sind getrennte Schlüssel) | Fills, Positionen, Orders, Bestände |
| **Binance** | `api_key`, `api_secret` | nur *Enable Reading*. Kein Trading, keine Auszahlungen | Fills, Orders, Bestände |
| **Binance USDⓈ-M Futures** | `api_key`, `api_secret` | *Enable Reading* plus Futures-Zugriff. Kein Trading, keine Auszahlungen | Fills, Positionen, Orders, Margin-Salden |
| **Bitget** | `api_key`, `api_secret`, `api_passphrase` | *Read-only*. Kein Trade, keine Auszahlung | Fills, Positionen, Orders, Bestände |
| **OKX** | `api_key`, `api_secret`, `api_passphrase` | nur *Read*. Kein Trade, keine Auszahlung | Fills, Positionen, Orders, Bestände |
| **OANDA** | `api_token` + die Konto-ID | Ein persönliches Zugriffstoken. **OANDA hat kein schreibgeschütztes Token**: Dasselbe kann handeln | Fills, Positionen, Orders, Bestände |
| **Coinbase Advanced Trade** | `api_private_key` + der vollständige Schlüsselname | CDP-Schlüssel, nur **View**, als **Ed25519** erstellt. Kein Trade, kein Transfer | Fills, Orders, Bestände |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` + die Konto-ID | OAuth-Anmeldung mit `ReadAccount`, `MarketData`, `openid`, `offline_access`. **Nicht `Trade`** | Fills, Positionen, Orders, Bestände |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | Dein Login plus der AppKey, den StoneX ausstellt. **Es gibt keine schreibgeschützten Zugangsdaten** | Fills, Positionen, Orders, Bestände |
| **Capital.com** | `api_key`, `identifier`, `api_password` | Ein API-Schlüssel (Zwei-Faktor muss an sein) und sein benutzerdefiniertes Passwort. **Keine schreibgeschützte Stufe** | Fills, Positionen, Orders, Bestände |
| **NinjaTrader** | `username`, `password`, `cid`, `sec` | Plattform-Login plus das Entwickler-API-Schlüsselpaar. **Kein schreibgeschützter Schlüssel** | Fills, Positionen, Orders, Bestände |
| **Kraken** | `api_key`, `api_secret` | *Query Ledger & Trade History* und *Query Open Orders* | Fills, Orders, Bestände |
| **Interactive Brokers (Flex)** | `flex_token` + eine Flex-Query-ID | Token des Flex Web Service: Er liest Kontoauszüge, er kann nicht handeln | Fills, Positionen, Bestände |

Geheimnisse sind nur schreibbar: Die App kennt immer nur, *welche* Namen gesetzt sind. Sie können eingetippt oder aus dem [Tresor](/de/config/settings#vault) eingesteckt werden, sodass ein Schlüssel mehrere Konten bedient.

### Hinweise pro Broker

- **Alpaca**: Die Einstellung *Umgebung* ist `live` oder `paper` und muss angegeben statt geraten werden, da beide auf verschiedenen Hosts mit verschiedenen Schlüsseln liegen. Alpaca meldet bei einem Fill keine Kommission, importierte Trades tragen also keine Gebühren: korrekt für kommissionsfreie Aktien, zu wenig für Krypto und Optionen, deren Gebühren als separate Kontoaktivitäten eintreffen.
- **Binance**, Spot wie Futures, beantwortet seinen Handelsverlauf **ein Instrument nach dem anderen**, ein Abruf muss die Instrumente also nennen (`BTCUSDT`, `ETHEUR`). Jeder andere Broker hier beantwortet das ganze Konto.
- **Drei Krypto-Handelsplätze enden bei 90 Tagen.** Bitget, OKX und Binance Futures liefern über die API drei Monate an Fills und nicht mehr; alles Ältere ist ein Download von ihrer Website. Das Import-Modal zeigt das Limit und lehnt einen Zeitraum ab, der davor beginnt, statt eine stille Halbantwort zu liefern.
- **Ein Derivatekonto ist kein Spot-Konto.** Bitget, OKX und Binance Futures können short sein, daher ist *Dieses Konto darf short gehen* dort standardmäßig angekreuzt. Entferne bei einem Bitget-Konto, das nur auf das Spot-Buch eingestellt ist, den Haken: Ein Spot-Verkauf ohne offene Position ist der Verkauf einer früher gekauften Münze, kein Short.
- **Ein Kontrakt wird in Kontrakten gezählt.** OKX meldet Derivate-Fills in Kontrakten und veröffentlicht, was einer wert ist (`ctVal × ctMult`), was aus seiner Instrumentenliste gelesen und als Punktwert des Trades übernommen wird. Bitget-USDT-M- und Binance-USDⓈ-M-Kontrakte sind in der Basismünze dimensioniert, bei ihnen ist er also eins. Coin-M-Kontrakte (inverse) werden nirgends gelesen: Sie sind in der Quote-Währung dimensioniert, und ihr PnL ist keine Menge mal ein Preis.
- **TradeStation ist der einzige mit OAuth.** Es gibt keinen statischen Schlüssel: Eine einmalige Browser-Anmeldung erzeugt ein Refresh-Token, das die App laufend gegen ein 20-Minuten-Zugriffstoken tauscht. Erteile bei dieser Anmeldung `ReadAccount`, `MarketData`, `openid` und `offline_access` und lass `Trade` weg; ein Token, das handeln könnte, wäre ein Dauerrisiko für nichts. Ein Fill ist hier ein *Order-Leg*, da TradeStation geschlossene Orders statt Ausführungen veröffentlicht, und die Identität, auf der ein erneuter Sync dedupliziert, ist die Order-ID plus die Stellung des Legs darin.
- **FOREX.com meldet sich an, es nutzt keinen Schlüssel.** Die Zugangsdaten sind Benutzername und Passwort des Kontos selbst plus der AppKey, den StoneX nach Unterzeichnung seiner API-Bedingungen ausstellt, derselbe Login kann also handeln: Behandle es als Geheimnis mit Vollzugriff und ändere das Passwort, wenn du das Konto entfernst. Ein von dort importierter Trade trägt **keine Kommission**, weil StoneX den Spread abrechnet; wird dein Konto stattdessen mit Kommission belastet, liegen die Zahlen hier unter der Wahrheit. Der Endpunkt für den Handelsverlauf nimmt kein Enddatum und keinen Cursor, ein weiter Zeitraum wird daher vorwärts in Seiten zu 200 durchlaufen.
- **Capital.com antwortet einen Tag nach dem anderen.** Sein Aktivitätslog begrenzt den Bereich zwischen zwei Daten auf 24 Stunden, ein Handelsjahr kostet also einen Aufruf pro Tag; Zeiträume über 400 Tage werden ausdrücklich abgelehnt, statt sie in das Rate-Limit laufen zu lassen. Jedes Instrument der Plattform ist ein **CFD**, ein Aktien-CFD wird also als Derivat abgelegt und nicht als Aktie, was ein Steuerformular verlangt. Eine Deal-ID benennt eine Position statt eines Fills, daher ist die Identität, auf der ein erneuter Sync dedupliziert, Deal, Zeitstempel und Richtung zusammen.
- **NinjaTrader erlaubt zwei Sitzungen pro Login**, und eine dritte schließt die älteste: Dieser Konnektor hält eine, eine daneben angemeldete Trading-Anwendung die andere. Seine Trading-API ist die Tradovate-Plattform, die es übernommen hat, weshalb die Fehler `tradovateapi` sagen. Er beantwortet die Fills, die seine Sitzung sehen kann, und veröffentlicht kein Tiefenlimit, prüfe also die älteste Zeile, die der erste Abruf liefert, bevor du dich für ein Steuerjahr darauf verlässt. Ein Futures-Punktwert wird aus dem Produkt des Kontrakts gelesen, nie aus der Wurzel angenommen.
- **OANDA gibt kein schreibgeschütztes Token aus.** Das persönliche Zugriffstoken, das dieses Konto liest, kann es auch handeln, was das Formular sagt: Behandle es als Zugangsdaten mit Vollzugriff und widerrufe es, wenn du das Konto entfernst. Nichts in der App nutzt es je zum Schreiben.
- **Interactive Brokers** ist überhaupt kein API-Schlüssel: Es liest einen gespeicherten Bericht über den Flex Web Service. Einrichtung, Zeitraumregel und Zeitzoneneinstellung stehen in [einem eigenen Abschnitt weiter unten](#interactive-brokers-the-flex-web-service).
- Rate-Limits sind die des Brokers, und jede Zeile zeigt das jeweils geltende. IBKR ist der strenge: Es baut den Kontoauszug auf Anforderung und lehnt eine zweite Anfrage ab, solange einer noch erzeugt wird.

## Eines verbinden

1. **Konto hinzufügen**, wähle den Broker und benenne es: Der Name ist, was die Modulauswahlen zeigen, *Kraken main* ist also besser als *Kraken 2*.
2. Fülle die Zugangsdaten aus oder wähle sie aus dem Tresor. Ein Konto, dem eines fehlt, wird als *unvollständig* angezeigt und von jedem Modul übersprungen, bis du es setzt.
3. Fülle die nicht geheimen Einstellungen, die der Broker braucht: eine Umgebung (Alpaca, Binance Futures, TradeStation, Capital.com, NinjaTrader), eine Konto-ID (OANDA, TradeStation, Capital.com, FOREX.com, NinjaTrader), den Coinbase-Schlüsselnamen, die IBKR-Query-ID und den Offset oder die Bitget-Bücher.
4. Wähle die **Module**, die es bedient, oder *alle Module*.
5. **Verbindung testen** erreicht den Broker und meldet, was geantwortet hat: Kontonummer und Status, wie viele Assets einen Saldo tragen oder welchen Kontoauszug das Flex-Token geliefert hat. Stimmt etwas nicht, nennt der Fehler die zu ändernde Einstellung.

Freigaben werden serverseitig durchgesetzt: Ein Modul, das nach einem Konto fragt, das ihm nie gegeben wurde, wird abgelehnt.

## Was es dir ermöglicht

Vier Ziele, eine Form. Was immer du importierst und wo es landet, ein Broker-Import läuft auf denselben zwei Schienen wie ein Datei-Import:

1. **Abrufen, dann ansehen.** Die App fragt den Broker, faltet die Antwort in das, was das Modul speichert (Positionen für das Journal, eine Bilanz für das Portfolio, geschlossene Veräußerungen für das Steuerformular), und zeigt es dir. In diesem Schritt wird nichts geschrieben, ein Abruf, der dir nicht gefällt, kostet also nichts.
2. **Festschreiben und den Faden behalten.** Alles Geschriebene trägt die ID dieses Imports und bleibt danach ein Objekt: *Rückgängig machen* löscht genau das, was es erstellt hat, und sonst nichts, *Behalten, nicht mehr verfolgen* trennt die Verknüpfung und lässt die Zeilen an Ort und Stelle. Beide liegen im Importverlauf des Moduls.

Duplikate sind Aufgabe der Schiene, nicht deine. Jede importierte Zeile erhält einen Fingerabdruck, sodass ein erneuter Abruf eines überlappenden Zeitraums erkennt, was schon abgelegt ist, es in der Vorschau markiert und nur einmal schreibt.

### Trades ins Journal importieren

**Journal → Import → Vom Broker abrufen**. Wähle das Konto, den Zeitraum und das Buch zum Ablegen, und die Fills kommen zu Positionen gefaltet zurück, in der Vorschau, bevor etwas geschrieben wird.

1. **Das Konto.** Nur die dem Journal gewährten werden aufgelistet, und ein unvollständiges sagt das. Das zuletzt genutzte Konto und der Zeitraum eines Buchs werden gemerkt, der nächste Abruf sind also zwei Klicks.
2. **Der Zeitraum**, per Datum oder mit den Chips *7 / 30 / 90 / 365 Tage*. Lies ihn als das Fenster, in das die *Fills* fallen, nicht das Fenster, in dem die Trades geschlossen wurden: Eine Position wird aus den Fills innerhalb des Zeitraums gefaltet, beginne also früh genug, um den Einstieg zu erfassen.
3. **Die Instrumente.** Binance beantwortet seinen Verlauf ein Instrument nach dem anderen, die Symbole sind dort also Pflicht, und *Vorschlagen* bietet an, was das Konto hält. Überall sonst ist das Feld ein Filter: Lass es leer für das ganze Konto.
4. **Shorts.** Ein Spot-Konto kann nicht short sein, daher wird ein Verkauf ohne offene Position als Zeilenfehler gemeldet, der die Lösung nennt (Zeitraum erweitern), statt in einen Phantom-Short verwandelt zu werden. Setze bei einem Margin-Konto den Haken bei *Dieses Konto darf short gehen*.
5. **Vorschau**, dann importieren. Die Zähler sind Fills, Trades, davon geschlossen und offen, plus was schon im Buch ist und was nicht gebaut werden konnte. Jede Zeile sagt vor dem Festschreiben, was sie ist.

- Dasselbe Ziel und dieselbe Faltung wie bei einem Datei-Import, minus der Zuordnung: Eine API antwortet mit typisierten Feldern, die Fragen, die eine CSV aufwirft (welche Spalte ist das Datum, ist das Dezimaltrennzeichen ein Komma), gibt es hier nicht.
- **Wiederholen ist sicher.** Die Identität einer Position ist ihr *eröffnender* Fill, das Erweitern des Fensters und erneutes Abrufen aktualisiert also, was seither geschlossen wurde, und lässt den Rest in Ruhe, statt denselben Trade zweimal abzulegen.
- **Eine Aktualisierung ist rein mechanisch.** Preise, Mengen, Gebühren und Daten kommen erneut vom Broker; deine Notizen, Tags, Strategie und Vorlagenfelder gehören dir und überstehen sie.

### Ein Portfolio an das angleichen, was das Konto hält

**Portfolio → Vom Broker**. Es liest eine **Bilanz**, keinen Handelsverlauf: Der Unterschied zu deinem Ledger wird Zeile für Zeile gezeigt, und du wählst, welche Zeilen angeglichen werden. Jede, die du annimmst, schreibt die eine Operation, die das Portfolio in Einklang bringt.

- Ein Symbol, das das Portfolio bereits hält, löst sich selbst auf; alles andere wird erfragt, denn „BTC“ an einer Börse ist ein String, und ein Asset ist hier eine Preisquelle.
- **Die Einstandsbasis wird nie erfunden.** Interactive Brokers veröffentlicht eine, und sie wird genutzt. Die Krypto-Börsen veröffentlichen eine Menge und sonst nichts, diese Zeilen sagen das also und setzen standardmäßig den heutigen Preis, den einen Preis, den niemand für eine Aussage über die Vergangenheit halten kann.
- **Den Preis von der Börse nehmen.** Bei einer Zeile ohne Einstandsbasis fragt ein Klick den Handelsplatz, zu welchem Preis das Asset gerade handelt, und trägt den Preis ein. Die Zeile nennt dann den Markt, der geantwortet hat (`BTCUSDT`, `XBT/USD`), und sagt es, wenn dieser Markt in etwas anderem als der eigenen Währung des Assets bepreist: Ein Binance-Preis ist in USDT, nicht in Dollar. Ein Asset, das der Handelsplatz nicht bepreist, wird in Ruhe gelassen, statt von anderswo bewertet zu werden, und du tippst den Preis selbst.

Alpaca, Binance (Spot und Futures), Bitget, Coinbase, Kraken, OKX, OANDA und TradeStation beantworten diese Frage. Interactive Brokers Flex nicht: Ein Kontoauszug ist kein Kurs-Feed, FOREX.com bepreist einen Markt über seine numerische ID statt über den Namen, den eine Portfoliozeile trägt, und NinjaTrader liefert Preise über eine separate Marktdaten-Berechtigung. Capital.com antwortet mit der Mitte der beiden Seiten, auf denen es handelt.

### Dein Buch im Chart zeichnen

**Chart → Broker-Buch**. Synchronisiere ein Konto, und seine Positionen und Orders im Markt werden als Preislevel im Chart des passenden Instruments gezeichnet, Durchschnittskurs für eine Position, Limit und Stop für eine Order. Der Abgleich läuft über den Ticker, Satzzeichen ausgenommen. Eine Position ohne Durchschnittskurs bekommt keine Linie und wird als solche gezählt.

### Ein Steuerjahr lesen

**Steuern → Vom Broker**. Ruft die Fills ab, faltet sie zu geschlossenen Positionen und summiert, was innerhalb des Steuerjahrs realisiert wurde, aufgeteilt auf die Kapital-, Derivate- und Krypto-Zeilen des Formulars. Es **schreibt nichts**: Du übernimmst die Zahlen ins Formular und speicherst das Szenario selbst.

- **Das Fenster ist nicht das Steuerjahr.** Was du im März verkauft hast, wurde früher gekauft, und ohne diesen Kauf gibt es keine Einstandsbasis: Verschiebe das Startdatum weit genug zurück, um ihn abzudecken. Eine Veräußerung, deren Kauf fehlt, wird gemeldet, nie gegen nichts bewertet.
- Jede geschlossene Position wird zum Kurs **ihres eigenen Ausstiegsdatums** umgerechnet. Eine ohne Kurs wird aufgelistet und aus den Summen ausgelassen, statt in der falschen Währung addiert zu werden.

## Interactive Brokers: der Flex Web Service {#interactive-brokers-the-flex-web-service}

IBKR ist der eine Broker hier, der nicht über eine Trading-API gelesen wird. Er wird über **Flex** gelesen, den Berichtsdienst der Kontoverwaltung: Du speicherst eine Query, die beschreibt, was in einem Kontoauszug stehen soll, und die App holt diesen Kontoauszug per HTTPS mit einem Token.

### Warum Flex und nicht TWS

Der [Marktdaten-Konnektor](/de/config/connectors#interactive-brokers) spricht den TWS-Socket zu einem Gateway, das du selbst betreibst. Dieser Socket ist das richtige Werkzeug für die Gegenwart und das falsche für die Historie: Er beantwortet offene Positionen und die Fills der **laufenden Sitzung**, *importiere meine Trades vom März* hat also gar keine Socket-Form. Flex liefert einen Zeitraum, und genau das fragt ein Import.

Der praktische Unterschied:

| | Flex Web Service | TWS- / IB-Gateway-Socket |
|---|---|---|
| **Was du betreibst** | nichts, es ist ein HTTPS-Aufruf | Gateway oder TWS, angemeldet, auf der Maschine |
| **Verlauf** | der Zeitraum der Query, bis zu einem Jahr zurück | nur die laufende Sitzung |
| **Zugangsdaten** | ein Token, das Berichte liest | deine Live-Sitzung, die handeln kann |
| **Hier genutzt für** | Journal-Import, Portfoliobestände, Steuerjahr, Chart-Positionen | Kurse, Charts, Live-Bars |

### Warum es der sichere Weg hinein ist

- **Das Token kann nicht handeln.** Es wird für den Flex Web Service ausgestellt, und dieser Dienst liefert Kontoauszüge. Dahinter steht kein Order-Endpunkt, den man zu deaktivieren vergessen könnte, kein Berechtigungs-Häkchen, das man falsch setzen kann. Vergleiche das mit einem Börsen-API-Schlüssel, bei dem Read-only ein Kästchen ist, an dessen Ankreuzen man denken muss.
- **Nichts bleibt lauschend zurück.** Kein laufendes Gateway, kein offener API-Port, keine Trusted IP zu deklarieren, nichts, was auf deiner Maschine wartet, während die App ruht.
- **Es läuft von selbst ab.** IBKR gibt einem Token eine Lebensdauer und schickt vor dem Ablauf eine Erinnerung per Mail. Ein vergessenes Token hört auf zu funktionieren, statt ewig gültig zu bleiben.
- **Die Query ist der Zaun.** Ein Token kann nur zurückgeben, was die gespeicherten Queries beschreiben. Beschränke die Query auf Trades und offene Positionen, und das ist alles, was die App je sehen kann, was immer sie anfragt.
- Wie alle Zugangsdaten hier ist das Token **in der App nur schreibbar**: Es kann eingetippt oder aus dem [Tresor](/de/config/settings#vault) eingesteckt werden und wird nie wieder angezeigt.

### Wie ein Abruf tatsächlich abläuft

1. Die App ruft `SendRequest` mit deinem Token und der Query-ID auf. IBKR antwortet mit einem Referenzcode und beginnt, den Kontoauszug zu **bauen**.
2. Sie fragt dann `GetStatement` mit diesem Code ab, bis das XML eintrifft, was normalerweise ein paar Sekunden dauert und bei einer weiten Query länger sein kann. Ein noch in Erzeugung befindlicher Kontoauszug ist die erwartete Antwort auf die ersten Versuche, kein Fehler.
3. Der Kontoauszug wird zu Fills (`Trade`-Zeilen auf Ausführungsebene) und Beständen (`Open Positions`) geparst, und die App filtert diese auf den gewählten Zeitraum.

Erzeugt IBKR nach einer Minute noch, sagt die App das, statt zu hängen: Verenge den Datumsbereich der Query oder versuche es gleich noch einmal.

### Einrichten

1. **Das Token**: Account Management → Settings → **Flex Web Service**. Erzeuge eines, kopiere es einmal, notiere das Ablaufdatum.
2. **Die Query**: Account Management → Performance & Reports → **Flex Queries** → neue *Activity*-Query. Schließe ein:
   - **Trades**, Detailgrad **Execution**, für das Journal und das Steuerformular;
   - **Open Positions**, für den Portfolio-Abgleich und das Chart-Overlay.

   Speichere sie und notiere die **Query-ID**, die Zahl neben ihrem Namen.
3. In OpenTraderWorld: Konto hinzufügen, Token einfügen, **Flex-Query-ID** und **Zeitversatz des Kontoauszugs** ausfüllen, dann **Verbindung testen**. Es meldet die Kontonummer, den Zeitraum, den der Kontoauszug abdeckt, und wie viele Trade-Zeilen er trägt, was der schnellste Weg ist zu sehen, dass der Query ein Abschnitt fehlt.

::: tip Zwei Einstellungen, die entscheiden, ob der Import stimmt
**Der Zeitraum ist der der Query, nicht deiner.** Eine Flex-Query trägt ihren eigenen Datumsbereich (*Last 365 Calendar Days*, *Year to Date*, ein eigenes Fenster), und der Webdienst nimmt überhaupt keine Daten. Die Daten, die du in der App wählst, **filtern**, was der Kontoauszug zurückgab, eine Query auf *Last 30 days* liefert also nie den März, egal wie weit zurück du fragst, und IBKR liefert nicht mehr als ein Jahr. Stelle die Query weit ein, filtere in der App.

**Ein Flex-Kontoauszug nennt nie seine Zeitzone.** Er stempelt die eigene Zone der Query und sagt nichts darüber, welche, setze also den *Zeitversatz des Kontoauszugs* auf diese Zone in Minuten (`-300` New York im Winter, `60` Paris), sonst landet jeder Fill in der falschen Stunde und Intraday-Trades am falschen Tag.
:::

### Was Flex nicht tut

- **Keine Orders im Markt**, das Chart-Overlay zeichnet IBKR-Positionen also zu ihrem Durchschnittskurs und keine Order-Level.
- **Keine Kurse.** Ein Kontoauszug ist kein Preis-Feed: Die Schaltfläche *Preis von der Börse nehmen* des Portfolios bieten die API-Broker, nicht dieser. IBKR ist der einzige der fünf, der eine **Einstandsbasis** veröffentlicht, die Zahl, auf die es bei einem Ledger ankommt.
- **Ein Kontoauszug auf einmal.** IBKR baut ihn auf Anforderung und lehnt eine zweite Anfrage ab, solange einer erzeugt wird, direkt aufeinanderfolgende Abrufe derselben Query warten also aufeinander.

## Modulfreigaben

| Modul | Was es liest |
|---|---|
| **Trading Journal** | die Fills des Zeitraums, für den Import |
| **Portfolios** | was das Konto hält |
| **Visualization** | Positionen und Orders im Markt, für das Chart-Overlay |
| **Tax Calculator** | die Fills eines Steuerjahrs |

Werden alle Module angekreuzt, fällt es zurück auf den Platzhalter *alle Module*, der auch in künftigen Versionen hinzukommende Module abdeckt.

## Grenzen

- **Ein Ticker wird nie geraten.** Ein Symbol, das die App nicht auflösen kann, ist ein Fehler, der die Lösung nennt, kein Best-Effort-Treffer.
- Was ein Broker nicht veröffentlicht, bleibt leer statt plausibel: keine erfundene Einstandsbasis, keine erfundene Gebühr, keine erfundene Seite.
- Das Löschen eines Kontos entfernt seine Zugangsdaten. Was es bereits importiert hat, bleibt.
- Broker-Konten sind im [Demo-Modus](/de/guide/demo) deaktiviert.
