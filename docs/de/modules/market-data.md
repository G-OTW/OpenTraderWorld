# Marktdaten & Backtesting

Diese Module bilden eine Kette: **Historical Data** lädt Kurshistorie in lokale Datensätze; **Backtest** und **Quant Tools** arbeiten mit diesen Datensätzen, und **Visualization** zeichnet jedes Instrument, das ein Konnektor bedient, gespeichert oder nicht. Alle drei setzen voraus, dass Historical Data installiert ist, da es den Datensatzkatalog besitzt, den sie lesen.

## Historical Data {#histdata}

Lade OHLCV-Candles von externen Anbietern in Datensätze, die in deiner Datenbank gespeichert werden, oder [importiere eine Datei](#import), die du bereits hast. Einmal gespeichert, gehören die Daten dir: charten, backtesten, exportieren, ohne erneutes Abrufen.

### Anbieter & Zugangsdaten

Anbieter werden einmal zentral als **[Daten-Konnektoren](/de/config/connectors)** konfiguriert: ein benanntes Anbieterkonto mit seinen Zugangsdaten, einem optionalen Anfragelimit und den Modulen, die es nutzen dürfen. Historical Data hat **keine eigenen Anbieter-Einstellungen**: Die Schaltfläche *Konnektoren* neben der Anbieterauswahl öffnet dieselbe gemeinsame Ansicht, die du in den Einstellungen findest.

Manche Anbieter sind **schlüssellos** (Binance und Binance Futures, Bitget, OKX, Kraken, Coinbase, Yahoo Finance) und funktionieren sofort; andere brauchen einen API-Schlüssel, und die meisten haben Gratis-Tarife. Ein Konnektor, dem die Zugangsdaten fehlen, zeigt *braucht Zugangsdaten* und wird übersprungen, bis du sie setzt.

Ausgehende Aufrufe werden unter **Einstellungen → API-Rate** gezählt, damit du deine Nutzung des Gratis-Kontingents beobachten kannst.

### Herunterladen

Wähle Anbieter, Anlageklasse, Timeframe, Ticker und Datumsbereich, dann **Herunterladen**. Hinweise:

- **Futures** nutzen Kontraktcodes: Basis + Monatsbuchstabe + Jahresziffer (`F G H J K M N Q U V X Z` = Jan…Dez), z. B. `GCJ5` für Gold April 2025.
- **Optionen** werden aus Basiswert, Verfall, Call/Put und Strike gebaut.
- **Intraday-Limits**: Anbieter liefern Intraday-Granularität nur für einen begrenzten Rückblick (z. B. ~7, 60 oder 730 Tage je nach Anbieter). Ältere Historie ist bei **1d / 1w** ohne Limit verfügbar. Das Formular warnt dich, bevor du einen unmöglichen Bereich einreihst.

::: warning Vor v0.0.15 heruntergeladene Datensätze
Vor v0.0.15 konnte ein Download oder ein Update, das die aktuelle Zeit erreichte, die noch entstehende Candle speichern, und spätere Updates begannen danach. Ein solcher Datensatz kann **unvollständige Candles** enthalten (High, Low, Close und Volumen abgeschnitten). Lade ihn erneut herunter, um sie zu ersetzen. Seit v0.0.15 wird eine Candle erst gespeichert, wenn ihre Periode beendet ist.

| Anbieter | Datensätze, die unvollständige Candles enthalten können |
|---|---|
| Coinbase, Kraken, Yahoo Finance, Alpaca, Massive, EODHD, Alpha Vantage, Capital.com, Interactive Brokers | vor v0.0.15 heruntergeladen oder aktualisiert |
| Binance (Spot) | vor v0.0.12 heruntergeladen oder aktualisiert |
| Binance USDⓈ-M, Bitget, OKX, OANDA, TradeStation, FOREX.com | keine |
:::

### Mehrere auf einmal

Ein Formular reiht einen ganzen Batch ein: Kreuze **so viele Timeframes** an, wie du brauchst, und tippe **mehrere Ticker durch Kommas getrennt** (`BTCUSDT, ETHUSDT, SOLUSDT`). Pro Paar aus Symbol × Timeframe wird ein Download eingereiht (3 Symbole × 2 Timeframes = 6 Downloads), alle über denselben Konnektor und denselben Datumsbereich.

Bevor du auf Herunterladen drückst, **beziffert das Formular den Batch**: wie viele Downloads es sind, ungefähr wie viele Anbieter-Anfragen das kostet und die Mindestdauer (sie laufen nacheinander, um die Rate-Limits des Anbieters einzuhalten). Trägt der Konnektor ein [Anfragelimit](/de/config/connectors#request-limits), zeigt ein kleines Messgerät, wie viel des aktuellen Fensters schon verbraucht ist, und die Zeile warnt dich, wenn der Batch darüber hinausgeht. Er wird nicht blockiert: Der Rest wartet, bis das Kontingent zurückgesetzt wird, und läuft von selbst weiter.

### Die Jobs beobachten

Downloads laufen als **Jobs** im Hintergrund, Chunk für Chunk, mit Live-Fortschritt. Filtere Jobs nach Status, Anbieter, Timeframe oder Ticker; ein Batch ist unter einer Kopfzeile gruppiert, die zeigt, wie viele seiner Downloads fertig sind.

- **Ein Job, der ein Limit erreichte, ist `wartend`, nicht fehlgeschlagen.** Die Zeile sagt warum (*Kontingent erreicht* oder *Anbieter-Rate-Limit*) und zählt bis zu dem Moment herunter, an dem er von selbst weiterläuft.
- **Abbrechen** stoppt jeden unfertigen Job, und ein Klick bricht **den Rest eines Batches** ab. Das Abbrechen ist kooperativ: Der Worker stoppt an der nächsten Chunk-Grenze, und die bereits geschriebenen Bars bleiben erhalten.
- Eine lange Pause und das Ende eines Batches lösen eine Benachrichtigung aus, an die [Kanäle](/de/config/settings#notifications) gepusht, die Historical Data gewährt wurden.

### Datensätze

Der Tab **Datensätze** listet alles Gespeicherte: Bar-Anzahl, Datumsbereich, Größe. Von hier aus kannst du:

- **Neueres holen**: Bars holen, die neuer sind als die zuletzt gespeicherte (einen Datensatz auffüllen).
- Als **CSV** oder **Parquet** **exportieren**. CSV öffnet sich in jeder Tabellenkalkulation; Parquet sind dieselben Bars typisiert und komprimiert, etwa ein Zehntel der Größe, von `pd.read_parquet` ohne Datums-Parsing oder Dtype-Raten gelesen. Die Parquet-Datei trägt außerdem Instrument, Timeframe und Quelle in ihren eigenen Metadaten, sodass das Zurückimportieren irgendwo in der App das Formular von selbst ausfüllt.
- Einen Datensatz **löschen** (verwirft alle seine Bars).
- Direkt zu einem **Chart** davon springen.

Downloads von Capital.com und OANDA speichern auch **Bid und Ask** jeder Candle; bei den anderen Anbietern holt ein Backtest sie, wenn er sie [braucht](#bid-ask-providers).

Importierte Datensätze stehen in derselben Liste, benannt nach der Herkunft ihrer Datei statt nach einem Anbieter. Sie tragen keine Schaltfläche *Neueres holen*: Es steht kein Anbieter dahinter, und einen zu erweitern geht nur über eine weitere Datei.

### Eigene Datei importieren {#import}

**Import** im Tab Datensätze liest Kurshistorie, die du schon hast: einen Börsen-Dump, einen Broker-Export, eine Tabelle, ein Anbieterarchiv. CSV, TSV, TXT, JSON oder **Parquet**, bis zu 20 MB, eine Zeile pro Bar.

Die Datei verlässt zwischen den Schritten nie deinen Browser und wird nie auf dem Server gespeichert: Jeder Schritt sendet sie erneut, es gibt also keinen halb fertigen Upload, der fortzusetzen oder aufzuräumen wäre.

**Die Spalten werden vorgeschlagen, du bestätigst sie.** Header werden gegen ein mehrsprachiges Wörterbuch (sechs Sprachen) *und* gegen das abgeglichen, wie die Werte tatsächlich aussehen, sodass `Date;Ouverture;Plus haut;…` und `open_time,open,high,low,close,volume` beide zugeordnet landen. Ein Punkt neben jeder Spalte zeigt, wie sicher der Detektor ist; was er nicht sicher weiß, überlässt er dir. Eine Spalte zu korrigieren lehrt ihn: Die nächste Datei mit diesem Header ordnet sich selbst zu.

Eine Parquet-Datei wird in dasselbe Raster gelesen wie eine CSV, Spaltenerkennung, Zuordnungsschritt und Vorschau funktionieren also identisch. Ihre Typen werden respektiert: Ein altes INT96-Timestamp (was Spark und ältere pandas schreiben) und ein `DATE` werden zu Daten, ein `DECIMAL` behält seine Skala, ein Null bleibt eine leere Zelle. Derselbe Reader bedient die Importe von Journal und Portfolio, auch diese akzeptieren also Parquet.

**Zeitstempel** werden als Daten oder als Unix-Ganzzahlen in Sekunden, Millisekunden, Mikrosekunden oder Nanosekunden gelesen, pro Spalte erkannt und überschreibbar. Ein Text-Zeitstempel ohne Zeitzone wird mit dem von dir gewählten Offset gelesen, der entscheidet, *zu welcher Periode* jede Zeile gehört, nicht bloß, wie sie angezeigt wird.

**Was fehlt, wird aufgefüllt, nie erfunden.** Eine Datei mit einer einzelnen Preisspalte ist eine Close-Serie (ein NAV, ein Indexstand): Open, High und Low werden aus dem Close gefüllt, was eine flache Bar ergibt, und das Formular sagt das. Eine Spalte, die du zugeordnet hast und die in einer Zeile leer ist, ist ein Fehler, der ihre Zeile nennt, keine Null.

Bevor etwas geschrieben wird, meldet die Vorschau über die ganze Datei: Bars, Zeilen, Spalten, das erste und letzte Datum, den Abstand, den deine Zeitstempel tatsächlich haben (als Timeframe angeboten), bei diesem Abstand fehlende Perioden, Zeilen, die sich eine Periode teilen, Bars, deren High/Low ihr Open/Close nicht einschließt, und jede Zeile, die nicht gelesen werden konnte.

**Was die Datei nicht sagen kann, sagst du.** Eine Datei sagt „Close“; sie sagt nicht, dass die Bars AAPL täglich sind. Der Import fragt also nach:

- **Ticker**, **Anlageklasse** und **Timeframe** (der Timeframe ist aus dem eigenen Abstand der Datei vorbefüllt).
- **Quelle**: der Broker, Handelsplatz oder Anbieter, von dem die Datei stammt, Freitext. Sie ist *Teil der Serienidentität*, sodass dasselbe Instrument, von zwei Brokern exportiert, zwei Datensätze bleibt statt zwei zu einem gemittelten Band.
- **Name** und **Tags**: deine eigenen Bezeichnungen, mit denen du die Serie im Katalog wiederfindest und filterst.

**Zweimal zu importieren ist sicher.** Ein erneuter Import landet im selben Datensatz und überschreibt Periode für Periode: gleiche Datei, gleiches Ergebnis. Überlappende Perioden werden in der Vorschau gezählt, bevor du festschreibst.

Eine von hier exportierte Parquet-Datei überspringt den Großteil dieses Formulars: Sie kennt ihren Ticker, ihre Anlageklasse, ihren Timeframe und ihre Quelle bereits und füllt nur die Felder, die du leer gelassen hast, sodass alles, was du getippt hast, weiter gewinnt. Exportieren, in pandas bearbeiten, zurückimportieren.

Einmal importiert, ist die Serie ein gewöhnlicher Datensatz: Backtests, Quant Tools, Journal-Anreicherung und die Faktor-Proxys des Portfolios lesen sie wie jeden heruntergeladenen.

### Ansehen, bevor du herunterlädst

Du musst keinen Job einreihen, um herauszufinden, ob ein Symbol das Speichern lohnt. Der Chart holt ein Fenster über einen Konnektor und **speichert nichts**; sieht das Fenster richtig aus, **speichere** es, und der normale Download-Job wird für genau diesen Bereich eingereiht. Das Speichern über bereits gehaltene Bars konsolidiert statt zu duplizieren, das Auffüllen eines Datensatzes aus dem Chart ist also sicher.

## Historical Data Visualization {#histviz}

Der Chart ist nicht an einen Datensatz gebunden: Er öffnet ein **Instrument**. Suche ein Symbol, wähle einen Timeframe, und die Bars kommen, ob du sie heruntergeladen hast oder nicht: Der Server liefert, was bereits in deinem Katalog liegt, und holt nur die fehlenden Ränder über einen [Konnektor](/de/config/connectors). Es wird nichts geschrieben, solange du es nicht verlangst.

Die Seite ist ein **Arbeitsbereich**: ein Raster aus Charts, eine Instrumentenliste daneben und eine Quick-Backtest-Sitzung darunter. Alles Folgende beschreibt einen einzelnen Chart, sofern nichts anderes dasteht; das Raster selbst steht unter [Arbeitsbereiche](#workspaces).

### Ein Instrument finden

Nichts wird über die Candles geschrieben, das nicht zu ihnen gehört: Das **Symbol oben links in einem Chart ist eine Schaltfläche**, und sie öffnet die Instrumentenauswahl als Modal, ein Suchfeld über jeden Konnektor, den der Chart nutzen darf. Tippe `BTC`, und Binance, Bitget, OKX, Kraken, Coinbase, Yahoo, EODHD, Alpha Vantage, Alpaca und Massive antworten gemeinsam, jeder Treffer mit dem Konnektor beschriftet, der ihn lieferte. Filtere nach Anlageklasse; hake Quellen im selben Modal an oder ab (das Häkchen **ist** die Freigabe, und der Server lehnt einen Konnektor ab, der diesem Modul nie gewährt wurde).

Bei leerem Feld listet das Panel, was du **zuletzt** gechartet hast, dann was bereits **gespeichert** ist, beides mit einem Klick zu öffnen und kostenlos.

Eine Zeile, die du nicht chartern kannst, sagt das anstelle des Charts und nennt den Grund: Der Konnektor wurde nie gewährt, der Anbieter kennt das Symbol nicht, eine Zugangsangabe fehlt oder kein gewährter Konnektor bedient es überhaupt. Jede Meldung trägt die Schaltfläche, die es behebt.

### Arbeitsbereiche {#workspaces}

Ein Arbeitsbereich ist ein **Raster aus Charts**, von 1x1 bis 3x4. Zeilen und Spalten werden in der Werkzeugleiste gewählt, ein vertikaler Split, ein horizontaler und ein 2x2 sind also dasselbe Bedienelement statt einer Liste benannter Layouts; ziehe an den Teilern, um einem Chart mehr Platz zu geben, oder maximiere einen Chart und kehre zum Raster zurück. Führe so viele Arbeitsbereiche, wie du magst, benenne sie und wechsle über die Auswahl; der zuletzt geöffnete kommt beim Neuladen zurück.

Jeder Chart trägt sein eigenes Instrument, seinen Timeframe, Zeichenstil, seine Indikatoren und Zeichnungen. Ein Chart wird in seiner eigenen Kopfzeile geschlossen, und eine leere Zelle fragt nach einem Instrument.

**Link-Gruppen.** Klicke die Link-Schaltfläche eines Charts, um ihm eine Farbe zu geben. Charts mit derselben Farbe teilen **Symbol** und **Fadenkreuz**, und auch die sichtbare Spanne, wenn sie denselben Timeframe haben. Der Timeframe selbst wird bewusst nie geteilt: Drei Panes auf einem Symbol bei 1m, 1h und 1d sind überhaupt der Grund, sie zu verknüpfen.

**Eine Verbindung für alle.** Panes am selben Konto teilen einen einzigen Live-Stream, vier Charts auf einem Alpaca-Schlüssel verbrauchen also einen Verbindungsplatz, nicht vier.

### Die Instrumentenliste {#rail}

Eine Leiste am linken Seitenrand, aus zwei Quellen, die nie vermischt werden:

- **Chart-Listen** gehören der Leiste selbst, gebaut mit der Schaltfläche `+`, die dieselbe Instrumentenauswahl öffnet. Sie halten Chart-Koordinaten, ein Klick chartet sie also ohne Abfrage. **Zuletzt** ist dasselbe ohne Namen.
- **Die Listen des Watchlists-Moduls** halten Quote-Symbole, sie zu chartern ist also eine Abfrage. Kursiert die Liste oder die Zeile über einen Daten-Konnektor, wird nur dieser Konnektor gefragt: Ein über IBKR kursiertes Symbol chartet auf IBKR oder gar nicht.

Klicke eine Zeile an, um sie im aktiven Pane zu chartern, oder ziehe sie auf ein beliebiges Pane. **Eine Watchlist wird von hier aus nie beschrieben**: Das Bearbeiten kopiert sie zuerst in eine Chart-Liste und bearbeitet die Kopie, und eine Chart-Liste in eine echte Watchlist zu verwandeln ist die Schaltfläche **Befördern** und sonst nichts.

### Historie laden

Der Chart öffnet auf den neuesten **1500 Bars** und setzt eine Schaltfläche an den linken Rand der geladenen Daten. Jeder Klick geht ein Stück weiter zurück. Keine Anbieteranfrage geschieht je ohne eine Geste von dir, und das hält einen kontingentierten Schlüssel berechenbar; der Gang endet, wenn die Historie des Anbieters ausgeht.

Das Dropdown **Timeframe** bietet jede Bargröße, die der Konnektor unterstützt, nicht nur die, die du zufällig heruntergeladen hast, und der Wechsel des Timeframes **behält die Daten, die du gerade ansahst**.

### Live-Streaming {#live}

Bei einem streamfähigen Konnektor **geht der Chart von selbst live**, sobald die neueste Bar die ist, die gerade entsteht: Die letzte Candle aktualisiert sich an Ort und Stelle, und das Bedienelement zeigt den Verbindungsstatus und die Verzögerung gegenüber der Börse. Dasselbe Bedienelement stoppt und startet den Feed von Hand.

Live wird nach **Instrument** adressiert, nicht nach Datensatz, und **speichert nichts**. Jedes Symbol, das ein Streaming-Anbieter bedient, kann live beobachtet werden, ohne es vorher herunterzuladen, und das ist der Punkt: Du kannst etwas ansehen, bevor du entscheidest, ob es das Behalten lohnt. Speichere das Instrument, während es live ist, und derselbe Feed beginnt, geschlossene Bars auch in den Datensatz aufzuzeichnen.

**Wer was streamt.** Binance (Spot und Futures), Bitget, OKX, Kraken und Coinbase streamen jeden Timeframe, den sie herunterladen, auch den täglichen, denn in einem 24/7-Markt ist die Tageskerze der Epochentag. Capital.com streamt ebenfalls jeden Timeframe, aus den Bid- und Ask-Candles, die es getrennt veröffentlicht, in ihrer Mitte gechartet. Alpaca, Massive und Interactive Brokers veröffentlichen **je eine Granularität** (Ein-Minuten-Bars, Ein-Minuten-Aggregate, Fünf-Sekunden-Bars), und der Chart faltet daraus deinen Timeframe. Das deckt jeden **Intraday**-Timeframe ab und überlässt täglich und wöchentlich dem Download: Eine Aktiensitzung sind nicht 1440 epochenausgerichtete Minuten, eine so gebildete Tageskerze würde also von der gespeicherten abweichen. Bei einem Timeframe, der nicht streamen kann, sagt das Bedienelement, welche es können, statt zu verschwinden. Die Details je Anbieter stehen unter [Live-Streaming](/de/config/connectors#live-streaming).

Ein Chart streamt ein Symbol, egal wie viele Panes: Mehrere Panes auf einem Instrument und mehrere Panes an einem Konto teilen die Verbindung, statt je einen Platz zu nehmen.

**Welches Konto.** Hält ein Anbieter mehr als einen Konnektor, bekommt das Live-Bedienelement eine Kontoauswahl. Zwei Schlüssel sind zwei Berechtigungen und zwei Verbindungsplätze, welcher verbraucht wird, ist also deine Wahl, kein Fallback. Das Umschalten startet den Feed auf dem anderen neu.

#### Keine Lücke an der Naht

Das Laden des Fensters, das Öffnen des Sockets und das Warten auf die Veröffentlichung des Anbieters brauchen alle Zeit, und ein Minuten-Bar-Anbieter spricht nur einmal pro Minute. Bis der erste Live-Tick eintrifft, kann der Chart eine oder mehrere Candles zurückliegen, und diese Candles fehlten früher, bis du neu geladen hast.

Der Stream sagt dem Server daher, wo der Chart endet, und der Server sendet, was dazwischen geschlossen hat: aus dem Katalog, wenn das Instrument gespeichert ist, vom Anbieter nur für den Rest, den er nicht beantworten kann, gar nichts, wenn es keine Lücke gibt. Was du beim Livegehen siehst, ist das, was der Markt getan hat, ohne Loch an der Verbindung.

#### Wenn es nicht laufen kann

Ein abgelehnter Schlüssel, ein Tarif ohne Streaming, ein Instrument, für das dein Konto kein Abo hat: Das sind Antworten, keine Fehler. Der Chart nennt, welche es ist, zitiert die eigenen Worte des Anbieters und **stoppt**, statt sich ewig hinter einem Punkt, der nie grün wird, neu zu verbinden.

| Was du siehst | Was es bedeutet | Was zu tun ist |
|---|---|---|
| *Not authorized* / abgelehnter Schlüssel | die Zugangsdaten sind falsch, oder der Tarif hat keinen Live-Feed (ein kostenloser Massive-Schlüssel lädt Historie und wird beim Live-Login abgelehnt) | den Konnektor korrigieren, dann *Erneut versuchen* drücken |
| *No subscription* für dieses Symbol | das Konto ist verbunden, aber für den Feed dieses Instruments nicht berechtigt (Alpaca gratis streamt IEX, nicht SIP oder OPRA; IBKR liefert, was du abonnierst) | ein anderes Instrument wählen oder das Abo beim Anbieter hinzufügen |
| *Connection taken* | die meisten Anbieter erlauben eine Live-Verbindung pro Konto, und ein anderes Programm hält den Platz | das andere Programm schließen; dieses versucht es von selbst weiter, da es sich von allein löst |
| *This timeframe does not stream* | der Anbieter veröffentlicht eine einzelne Granularität, und dein Timeframe liegt darüber | auf einen der vom Bedienelement gelisteten Timeframes wechseln oder bei heruntergeladenen Daten bleiben |
| *Connector not granted* | dem Chart wurde dieser Konnektor nie gegeben | ihn unter [Daten-Konnektoren](/de/config/connectors) gewähren, über den Link in der Meldung |
| *Market data lines* fast aufgebraucht | Interactive Brokers begrenzt, wie viele Symbole ein Konto gleichzeitig streamt, und der Arbeitsbereich ist nahe an dieser Grenze | ein Pane schließen oder den Live-Feed eines nicht beobachteten stoppen, bevor der nächste Chart ohne Grund verstummt |

Alles andere (ein abgebrochener Socket, ein Anbieter-Schluckauf) verbindet sich still mit Backoff neu.

### Chart

- **Charttypen**: Candles, OHLC-Bars, Linie und **Renko** (mit Brick-Größe).
- **Indikatoren**: SMA, RSI, MACD und mehr, als Overlays oder separate Panes, jeweils mit konfigurierbarer Quelle, Linien-/Füllfarben und Breite. Jede Serie bekommt **ihre eigene Zeile in der Chart-Kopfzeile**, mit Ausblenden, Einstellungen und Entfernen beim Darüberfahren, und jedes Pane ist über der Zeichnung betitelt, die es hält. Die Kopfzeile **liest am Fadenkreuz**: O H L C, die Veränderung und der Wert jedes Indikators an der Bar unter dem Cursor, mit Rückfall auf die neueste sichtbare Bar, wenn der Cursor weg ist.
- **Chart-Einstellungen**: lineare oder logarithmische Skala, horizontale und vertikale Gitterlinien, **Tagestrenner**, der **vorherige Schluss** als Referenzlinie, Fadenkreuz und seine Wert-Tags pro Serie, Hover-Tooltip (standardmäßig aus), Auf-/Ab-Farben, Preisskala links oder rechts und ein **Last-Price-Tag** an der Preisachse, eingefärbt wie die Candle, die ihn erzeugte.
- **Die Navigation ist handgesteuert**: Ziehen schwenkt beide Achsen (Zeit seitwärts, Preis auf und ab), das Rad zoomt (pro Gerät kalibriert, ein Maus-Rastpunkt und ein Trackpad-Tick bewegen also gleich viel). Das Wechseln des Charttyps behält den aktuellen Zoom.
- **Volumen** wird unten im Preis-Pane gezeichnet, wie Marktterminals es zeichnen, statt in einem eigenen Pane. Dazu ein einzelner Vollbildmodus.
- **Micro-Cap-Preise** werden `0.0₅4549` geschrieben, wobei der tiefgestellte Index die Nullen zählt, statt einer Skala identischer `0.0000`-Beschriftungen.

### Zwei Instrumente vergleichen

Füge demselben Chart ein weiteres Instrument hinzu, und es wird **rebasiert** gezeichnet, denn zwei Preise in zwei Währungen auf einer Achse sagen nichts. Zwei Lesarten, einen Klick auseinander:

- **prozentuale Veränderung**, beide Serien auf den Fensterbeginn rebasiert, was beantwortet, *welches stärker gestiegen ist*;
- **Verhältnis**, dieses Instrument geteilt durch das andere, auf 100 rebasiert, was die Pair-Trade-Ansicht ist: Die Linie steigt, wenn das gechartete Instrument besser abschneidet.

Vergleichsserien werden **Periode für Periode** am Chart ausgerichtet, sodass zwei Märkte, die denselben Tag unterschiedlich stempeln, trotzdem übereinanderliegen.

### Den Chart speichern

- **Als Bild**: ein PNG des Charts genau so, wie er auf dem Bildschirm steht, Zeichnungen und Overlays eingeschlossen.
- **Als Seite**: eine HTML-Datei, die offline in jedem Browser öffnet und das Bild enthält, wovon es ein Bild *ist* (Instrument, Fenster, Timeframe, Indikatoren, Vergleiche, wer die Bars lieferte) und **die Bars selbst**, eingebettet. Ein in ein Dokument eingefügter Screenshot ist eine Behauptung, die später niemand prüfen kann; diese hier lässt sich erneut lesen. Nichts wird hochgeladen: Die Datei wird in deinem Browser gebaut.

### Eigene Indikatoren im Chart {#custom-indicators-on-the-chart}

Der Indikator-Dialog hat einen zweiten Tab: **Eigene**. Er enthält dieselbe Node-Graph-Bibliothek, aus der das Modul [Backtest](#strategies-and-custom-indicators) baut, ein Indikator existiert also **einmal**, und beide Module sehen dieselbe Definition. Wähle einen aus der Liste, um ihn zu zeichnen, oder baue hier mit demselben Builder einen neuen; das Speichern schreibt ihn in die gemeinsame Bibliothek zurück.

Anders als ein Katalog-Indikator **wählt ein eigener sein Pane selbst**: im Preis oder in einem eigenen Pane. Diese Wahl triffst du pro Instanz, derselbe Indikator kann also in einem Chart die Candles überlagern und in einem anderen darunter sitzen. Ein 0-100-Indikator als Overlay bekommt eine versteckte zweite Skala, damit er den Preis nicht plattdrücken kann.

Die Definition **reist mit der Instanz**: Ein Chart zeichnet seinen eigenen Indikator auch nach dem Löschen der Bibliothekszeile weiter, und das Neuladen aktualisiert ihn aus der Bibliothek, solange die Zeile existiert.

### Zeichenwerkzeuge

Eine Leiste am linken Rand des Charts: **Trendlinie**, **horizontale** und **vertikale** Linie, **Rechteck**, **Fibonacci-Retracement**, **Text**, **Long-** und **Short-Positionsboxen** (Einstieg, Ziel und Stop, mit dem resultierenden R:R) und eine **Messung**, die Preisänderung, Prozent, Anzahl der Bars und verstrichene Zeit meldet.

- Jedes Objekt hat seinen eigenen **Stil** (Farbe, Breite, Strichelung) und wird durch Ziehen seiner Griffe bearbeitet.
- Ein **OHLC-Magnet** rastet einen Griff auf Open, High, Low oder Close der Bar darunter ein, und ein nahe einem bereits im Chart liegenden Objekt abgelegter Griff rastet darauf ein, mit einer Hilfslinie, die sagt, was er erfasst hat.
- **Stilvorlagen**: Gestalte ein Objekt, speichere es unter einem Namen und wende es auf die nächsten an. Eine Vorlage kann zum Standard für jede neue Zeichnung werden.
- **Zwischen Charts kopieren**: <kbd>Strg/⌘+C</kbd> dann <kbd>Strg/⌘+V</kbd> fügt das gewählte Objekt ein, auch in ein anderes Instrument; <kbd>Strg/⌘+D</kbd> dupliziert es an Ort und Stelle, um eine Bar versetzt.
- Zeichnungen sind in **Zeit und Preis** verankert, nicht in Pixeln, sie bleiben also bei jedem Zoom, Schwenk oder Timeframe-Wechsel auf ihren Bars.
- Sie werden **pro Instrument** aufbewahrt, nicht pro Timeframe und nicht pro Pane: Eine auf dem 1h gezeichnete Trendlinie ist dieselbe Linie auf dem 15m, und zwei Panes auf einem Symbol zeigen ein Board.
- **Rückgängig** (die Schaltfläche der Leiste oder <kbd>Strg/⌘+Z</kbd>) nimmt die letzte Zeichnung oder die letzte Quick-Backtest-Order zurück.

#### Die Objektliste

Ab fünf Zeichnungen braucht ein Chart eine Liste, also gibt es eine: jedes Objekt auf diesem Instrument mit dem, was es ist, und dem Preis, an dem es sitzt, und den vier Dingen, die es dann braucht: **ausblenden**, **sperren**, **löschen** und **umordnen**. Die Reihenfolge ist die Malreihenfolge, die entscheidet, was oben liegt. Das Wählen einer Zeile wählt sie im Chart, und die Liste ist dasselbe Array, das der Chart zeichnet, eine Änderung zeigt sich also, bevor der Dialog schließt.

### Alarme {#alerts}

Ein Preis oder ein Indikator-Level, **vom Server überwacht**. Der Browser kann geschlossen sein, die Maschine etwas anderes tun: Der Alarm feuert trotzdem, in deine Benachrichtigungen und in die [Kanäle](/de/config/settings#notifications), die dem Chart gewährt sind (keiner angehakt = alle).

Setze einen über den Alarm-Dialog oder über eine bereits gezeichnete horizontale Linie, die ihren Preis übergibt.

Drei Regeln sind wissenswert, weil sie Entscheidungen sind und keine Details:

- **Nur geschlossene Bars.** Das High einer entstehenden Candle ist noch keine Tatsache, der nächste Tick kann es revidieren. Ein Alarm, der darauf feuerte, würde etwas melden, das nie passiert ist.
- **Ein Kreuzen, kein Zustand.** *Kreuzt nach oben* wartet, bis der Preis das Level beim Steigen **durchquert**, ein Alarm unter dem aktuellen Preis feuert also nicht in dem Moment, in dem du ihn anlegst.
- **Das Level wird auf dem Timeframe dieses Charts gelesen**, und ein Tagesalarm wird weit seltener neu gelesen als ein Ein-Minuten-Alarm: Ein nicht gespeichertes Instrument kostet pro Prüfung eine kleine Anfrage, und eine Bar, die sich einmal am Tag bewegt, verdient nicht eine pro Minute.

Jeder Alarm kann **einmal** oder jedes Mal feuern, mit Cooldown. Die Liste sagt, wann jeder zuletzt feuerte, was er zuletzt las und, wenn ein Konnektor oder ein Kontingent im Weg steht, warum er überhaupt nicht laufen konnte. Alarme werden aus dieser Liste pausiert und wieder scharf geschaltet.

### Broker-Buch {#broker-book}

Synchronisiere ein [Broker-Konto](/de/config/brokers), und der Chart zeichnet, was du tatsächlich hältst: eine Preislinie pro offener Position zu ihrem Durchschnittskurs, eine pro Order im Markt bei ihrem Limit oder Stop. Level landen auf dem Chart, dessen Ticker passt, Satzzeichen ausgenommen, sodass ein Arbeitsbereich mit mehreren Instrumenten sich selbst beschriftet. Nur lesend und nur beim Drücken von *Sync* neu gelesen: Eine Position ohne Durchschnittskurs bekommt keine Linie und wird als solche gezählt, statt irgendwo plausibel platziert zu werden.

### Quick-Backtest

Ein Schmierblatt, um einen Chart von Hand zu handeln: **klicke in den Chart, um eine Position zu eröffnen, klicke erneut, um sie zu schließen**. Drücke stattdessen lange, um Seite und Größe zu wählen, und klicke den Pfeil einer Markierung, um sie umzudrehen. Pyramidisieren, Teilschließungen und Umkehrungen ergeben sich alle aus den Fills, die du platzierst.

Die Sitzung erstreckt sich über **den ganzen Arbeitsbereich, nicht einen Chart**: Jeder sichtbare Chart postet seine Fills hinein, und die Zahlen sind ihre Summe, so wie ein Buch aus mehreren Instrumenten sich tatsächlich liest. Die Auswahl im Panel benennt den **Ziel-Chart**, auf dem ein Klick einen Trade platziert und den das Größenfeld bearbeitet; er ist das aktive Pane, die Wahl hier und das Klicken dort sind also derselbe Akt.

Der Streifen unter dem Arbeitsbereich ist eingeklappt eine Zeile mit Sitzungszahlen. Aufgeklappt ist er an seiner Oberkante größenveränderbar und hat drei Tabs:

- **Trades**: die Trade-Liste der Sitzung mit Einstieg, Ausstieg, P&L und R (gemessen am schlechtesten offenen Verlust des Trades, da es keinen Stop zum Zitieren gibt), plus Zurücksetzen.
- **Statistiken**: Trefferquote, Expectancy, Profit Factor, Ergebnisse pro Seite, Serien und eine Verteilung der Renditen.
- **Performance**: die P&L-Kurve der Sitzung, wobei jeder Trade sein Run-up und seinen schlechtesten offenen Verlust trägt, sodass ein Gewinner, der den Tag unter Wasser verbrachte, als solcher gelesen wird.

Die Größe wird in **Einheiten**, **Kontrakten** (mit einem Punktwert multipliziert) oder **Notional** eingegeben und zum Fill-Preis umgerechnet. Diese Fills leben **in deinem Browser, pro Instrument**: Es ist ein Schmierblatt zum Lesen eines Charts, nie Journal-Daten, und nichts wird an das [Trading Journal](/de/modules/journal) gepostet.

Die Schaltfläche **Backtest** übergibt dasselbe Instrument an das Modul [Backtest](#backtest) und speichert es vorher, falls es nicht gespeichert war.

### Der Chart merkt sich, wo du aufgehört hast

Charttyp, Indikatoren und Zeichnungen gehören zum **Instrument**, nicht zu einem Datensatz und nicht zu einem Pane: Sie werden kurz nach jeder Änderung serverseitig unter den eigenen Koordinaten des Symbols gespeichert und kommen in jedem Pane, in jedem Arbeitsbereich, aus jedem Browser genauso zurück. Das gilt auch für ein Symbol, das du nur einmal angesehen und nie heruntergeladen hast, und das ist der Punkt: Etwas zu chartern, das du nicht behalten wolltest, heißt nicht mehr, zu verlieren, was du darauf gezeichnet hast.

Was *nicht* serverseitig gespeichert wird, ist die Quick-Backtest-Sitzung, die in diesem Browser bleibt.

Der Schalter **Daten automatisch speichern** in den Chart-Einstellungen ist eine eigene Entscheidung, über die Bars statt das Layout: Er speichert ein Instrument beim ersten Chartern, was einen Download einreiht. Er ist standardmäßig aus.

### Wenn Daten fehlen

Ein Fenster, das zu kurz zurückkommt, sagt immer **warum**, in einem Hinweis über dem Chart, und behält die Bars, die ankamen:

| Grund | Was passiert ist |
|---|---|
| **auth** | Die Zugangsdaten des Konnektors fehlen oder wurden abgelehnt. |
| **quota** | Der Konnektor hat sein eigenes [Anfragelimit](/de/config/connectors#request-limits) erreicht. |
| **rate_limit** | Der Anbieter hat die Anfrage gedrosselt. |
| **symbol** | Der Anbieter kennt diesen Ticker nicht. |
| **depth** | Der Anbieter liefert bei diesem Timeframe keine Historie so weit zurück. |
| **provider** | Alles andere, was der Anbieter zurückgab. |

## Backtest {#backtest}

*Kombiniere Indikatorsignale, dimensioniere mit Pyramidisierung, miss den Edge.* Wähle einen Datensatz oder ein ganzes Portfolio, definiere Regeln, führe aus. Kein Code.

### Strategie

- **Einstiegs- / Ausstiegsregeln** pro Seite, aus Vergleichen zwischen Indikatoren, Preis und festen Werten gebaut. Gruppiere Regeln mit **UND** (alle müssen gelten) oder **ODER** (eine genügt).
- **Richtung**: long, short oder beides. Optionen: die Short-Seite als Spiegel der Long-Seite ableiten (inverse Operatoren, Oszillator-Level gespiegelt: RSI unter 30 wird zu RSI über 70; ADX-, ATR- und Volumenfilter bleiben, wie sie sind), und **Stop & Reverse** (die Position umdrehen, wenn das Gegensignal feuert).
- **Stop-Loss / Take-Profit** pro Seite: Jeder ist ein Kästchen, das du unabhängig ein- oder ausschaltest (Prozent des durchschnittlichen Einstiegs oder ein Vielfaches der ATR der letzten Candle, die vor dem Einstieg schloss). Ohne Ausstiegsregeln erfolgen Ausstiege über SL/TP oder Umkehr.

### Sizing, Konto & Kosten

- Dimensioniere nach **Prozent des Eigenkapitals** oder **fester Menge/Lots/Kontrakte**, mit **Hebel** und **Startkapital**. Eine feste Menge **skaliert mit dem Hebel**, nach der Retail-Konvention, ein Hebel von 3 bei fester Größe 1 eröffnet also 3 Einheiten.
- **Pyramidisierung**: bis zu N gestapelte Einstiege erlauben, wenn das Einstiegssignal erneut feuert; SL/TP folgen dann dem durchschnittlichen Einstiegspreis. Ein Zukauf wird zur Eröffnung gesendet, bevor die Candle getestet wird: Eine Candle, die dann den Stop trifft, schließt die vom Zukauf gemachte Position, und ein vom Zukauf verschobener Stop (Breakeven) wird auf derselben Candle getestet.
- **Kosten**: Gebühr (fest oder % des Notionals, pro Trade oder pro Einheit) und **Spread %**, damit die Ergebnisse keine Fantasie sind. Eine Gebühr kann negativ sein, für einen Maker-Rebate oder einen Broker, der pro Fill zahlt. Die Einstiegsgebühr verlässt das Cash beim Fill, wie ein Broker sie belastet, sodass Eigenkapital, Drawdown und Zukäufe einer offenen Position netto davon sind.
- Einstellungen ohne Bedeutung (kein Kapital, eine Größe oder ein Hebel von null oder darunter, ein Kontrakt ohne Wert, ein nicht handelbares Gitter) werden vor dem Lauf abgelehnt, ebenso ein Datensatz mit einer Candle, die keine ist (ein High unter dem Low, ein Preis, der keine Zahl ist), wobei diese Candle benannt wird.

### Sizing (erweitert)

Jenseits von Prozent des Eigenkapitals und fester Menge:

- **Risiko pro Trade**: so dimensionieren, dass ein Stop-Loss-Treffer einen festen % des Eigenkapitals kostet (braucht einen Stop auf der gehandelten Seite).
- **Fraktionales Kelly**: Dimensioniere aus Trefferquote und Payoff der letzten *N* Signal-Trades der Strategie, übersprungene eingeschlossen (ein Breakeven-Trade ist weder Gewinn noch Verlust), skaliert mit dem von dir gewählten Bruchteil und gedeckelt; bis das Fenster sich füllt, wird eine Warm-up-Größe genutzt. Eine Verlustphase pausiert Einstiege, ohne sie für immer zu stoppen: Sie setzen wieder ein, sobald der Edge zurückkommt.
- **Eigenkapitalstufen**: eine Tabelle von Schwellen; die höchste Stufe, deren Level ≤ aktuelles Eigenkapital ist, legt die Größe fest.

### Portfolio (Multi-Asset)

Füge mehrere Datensätze hinzu und führe eine Strategie über alle auf einer **zusammengeführten Uhr** aus (alle auf denselben Timeframe festgelegt):

- Eine **Ausrichtungsvorschau** zeigt die Länge der zusammengeführten Uhr, das überlappende Fenster, Indikator-Warm-up-Bars (einschließlich des kumulierten Rückblicks eines verketteten eigenen Indikators) und fehlende Bars pro Asset, alles vor der Simulation.
- **Portfolio-Limits**: begrenze die Zahl offener Positionen und das Gesamt-/Pro-Asset-Exposure. Eine Position, die zu einer Eröffnung gehalten wird, behält dort ihren Platz, auch wenn sie später in dieser Candle schließt.
- **Sitzungen**: Assets, deren Candles zu unterschiedlichen Stunden öffnen (ein Krypto-Tag um 00:00 UTC, ein New Yorker um 13:30), handeln in dieser Reihenfolge. Eine Order bei der früheren Eröffnung dimensioniert und prüft ihre Limits am vorherigen Schluss des späteren Assets, nicht an einer Eröffnung, die noch nicht stattgefunden hat.
- Eine **Aufschlüsselung pro Asset** meldet Trades, Netto-PnL, Gebühren, Trefferquote und Exposure je Instrument.

### Grid-Strategie

Eine Leiter von Preislevels zwischen einer unteren und oberen Grenze; jede Zelle kauft tief und verkauft am nächsthöheren Level, **long**, **short** oder **neutral**. Dimensioniere eine feste Menge pro Level oder teile ein Gesamtbudget auf die Zellen auf, mit optionalen Stops über/unter der Leiter. Ergebnisse melden Fills, Round Trips und den End-Bestand.

- **Neutral** handelt beide Seiten der Mittellinie: Die Zellen darunter kaufen und verkaufen ein Level höher, die Zellen darüber verkaufen short und kaufen ein Level tiefer zurück. Es braucht eine ungerade Zahl von Leveln.
- Ein Kauf liegt nur auf einer Linie **unter** dem letzten Schluss (ein Verkauf darüber) und füllt an der Linie oder zur Eröffnung, wenn die Candle darüber hinaus öffnet. Ziele füllen genauso.
- Ein **Stop** unter oder über der Leiter füllt an seinem Level (zur Eröffnung bei einer Lücke), nach den Fills, die der Kurs auf dem Weg dorthin traf, und hält dann das Grid an.
- Ohne Grenzen erstreckt sich die Leiter über die bisher bekannte Spanne: das tiefste Tief und höchste Hoch der Candles vor jeder.
- Außerhalb des Handelsfensters mit *schließen* geht der Bestand zur Eröffnung, vor allem anderen in der Candle.

### DCA (Sparplan)

Ein dritter Modus neben Signalregeln und dem Grid, für die Art, wie das meiste Geld tatsächlich angelegt wird: ein **gewichteter Korb**, über die Zeit gekauft, nie rebalanciert.

- **Gewichte, fest.** Jeder eingesetzte Euro wird nach den Gewichten aufgeteilt, die du pro Ticker setzt. Nichts wird rebalanciert, eine Regel, die bei einem von fünf Assets feuert, setzt also den eigenen Anteil dieses Assets ein.
- **Geld hinein.** Das Startkapital wird in einem Zug bei der ersten Bar jedes Assets gekauft. Alles danach ist **neues Geld**: ein wiederkehrender Beitrag (pro Bar, Tag, Woche, Monat, Quartal oder Jahr, bei Eingang investiert oder als Cash gehalten) und Kaufregeln, die einen Betrag einzahlen, wenn ihre Bedingung gilt.
- **Kaufregeln**: ein fester Betrag, ein % des Cash, des Portfolios oder der Einstandsbasis, mit einer maximalen Zahl an Auslösungen und einem Cooldown, gefüllt zur Eröffnung der nächsten Bar.
- **Verkaufsregeln**: ein % der Position, die ganze Position, eine Zahl Einheiten oder ein Betrag, ausgelöst durch ein **Gewinnziel**, eine Bedingung oder beides, mit dem Erlös als Cash gehalten oder entnommen.
- **Bedingungen** sind die gewöhnlichen Regelgruppen der Engine plus zwei für diesen Modus geschriebene Familien: **Marktkennzahlen** (Rückgang vom Hoch, Anstieg vom Tief, Veränderung über N Bars, Veränderung seit Beginn) und die **Live-Position** (P&L %, Drift seit dem letzten Kauf, Durchschnittskurs, Einheiten, Wert, Gewicht %, Cash %, Drawdown). Sie werden standardmäßig auf einem **gewichteten Korbindex** ausgewertet oder pro Asset, was dann nur die Assets kauft, die gelten.
- **Kennzahlen für einen Sparplan**, nicht für eine Strategie: Drawdown und Sharpe auf der **einzahlungsbereinigten (zeitgewichteten)** Kurve, damit eine Einzahlung nicht als Rally gelesen wird; Rendite auf das eingezahlte Geld; **IRR** für die kapitalgewichtete Rendite; und ein Benchmark mit demselben insgesamt eingezahlten Betrag, in einer Summe zu Beginn angelegt.

Sizing, Pyramidisierung und Stops gelten hier nicht: Die eigenen Regeln des Plans entscheiden jeden Fill.

### Handelsfenster

Ein Schritt **Filter** entscheidet, *wann* die Strategie eröffnen darf, auf der von dir gewählten Uhr: eine benannte Zeitzone (`America/New_York`), die der Sommerzeit folgt, oder ein fester UTC-Offset, der das nicht tut:

- **Wochentage** und **Sitzungen** (mehrere pro Tag, ein Ende vor dem Start läuft über Mitternacht).
- **Kalender**: nur an bestimmten Daten handeln oder nie an ihnen. *Nie* schlägt *nur*.
- Außerhalb des Fensters wird die Position entweder **gehalten** oder **geschlossen**, und Pyramiden-Zukäufe können ebenfalls blockiert werden. Einstiege werden gesperrt; Ausstiege, Stops und Take-Profits laufen auf jeder Bar weiter.

### Kosten & Ausführungsrealismus

- **Slippage**: eine feste Zahl Ticks oder ein Prozent des Preises, auf jeden Fill angewendet.
- **Funding**: ein konstanter Jahressatz auf das offene Notional für Perp-Schätzungen (Longs zahlen, Shorts erhalten).
- **Circuit Breaker**: Handel anhalten nach einem maximalen Tagesverlust (für den Tag) oder einem maximalen Drawdown (für den Lauf).
- **Instrumentenprofil**: Preis-Tick, Lot-Schritt, Mindestmenge und Kontraktmultiplikator, damit Größen und Preise auf einen realistischen Kontrakt einrasten. Jeder Fill und jeder Stop, jedes Ziel, Limit und jede Grid-Linie wird auf den Tick gerundet, gegen den Trader: Ein Kauf zahlt den Tick darüber, der Stop eines Longs sitzt einen Tick tiefer.

### Order-Ausführung {#execution}

Wie Stops und Ziele immer füllen:

- Ein Trade wird **auf der Candle, in der er eröffnet**, gegen seinen Stop und Take-Profit getestet, nicht erst ab der nächsten.
- Der **Take-Profit ist eine Limit-Order**: Er füllt am Ziel, ohne Slippage und ohne berechneten Spread, sobald die handelnde Seite es erreicht (Bid für einen Long, Ask für einen Short), oder erst, wenn der Preis es **durchhandelt**, falls du das wählst (Erweitert, *Ausführung*). Eine Candle, die **über das Ziel hinaus eröffnet**, füllt es zu dieser Eröffnung, vor allem anderen in der Candle.
- Der **Stop-Loss ist eine Stop-Order**: Er füllt am Stop oder zur Eröffnung, wenn die Candle darüber hinaus gappt, und zahlt Spread und Slippage.
- Erreicht eine Candle **sowohl** den Stop als auch das Ziel und nichts anderes sagt, was zuerst kam, gewinnt der Stop.
- Ein innerhalb einer Candle geschlossener Trade (Stop, Ziel, Limit) wird nicht zur Eröffnung dieser Candle neu eröffnet, einem Preis von vor dem Ausstieg: Ein neuer Einstieg wartet auf die nächste Candle.
- Jedes bei einem Schluss entschiedene Signal wird zur **nächsten Eröffnung** ausgeführt: ein Einstieg, die Ausstiegsbedingung und *Ausstieg, wenn der Einstieg nicht mehr gilt*. Nichts füllt zum Schluss, der seine eigene Entscheidung erzeugte.
- MAE und MFE zählen nur, was der Trade erlebt hat: von seinem Fill bis zu seinem Ausstieg, nie den Rest der Candle, nachdem er sie verließ.

Optionen im Schritt **Erweitert** (*Ausführung*), alle standardmäßig aus:

- **Einstiegs-Order**: Market oder **Limit**. Ein Limit liegt zum Kaufen unter und zum Verkaufen über der Referenz, mit einem Offset (Prozent, Preisabstand oder ATR-Vielfaches) vom Schluss der Signal-Candle oder der Eröffnung der nächsten Candle. Es bleibt für die von dir gesetzte Anzahl Candles gültig, füllt bei Berührung oder nur, wenn der Preis es durchhandelt, und füllt zum eigenen Preis ohne Slippage (zur Eröffnung, wenn eine Candle darüber hinaus öffnet). Ein Signal, das weiter gilt, bewegt keine ruhende Order. Pyramiden-Zukäufe folgen derselben Regel.
- **Ausstiegs-Order**: dieselbe Wahl für Signal-Ausstiege (die Ausstiegsbedingung und *Ausstieg, wenn der Einstieg nicht mehr gilt*). Ein Ausstiegs-Limit, das nicht rechtzeitig füllt, **geht entweder zur nächsten Eröffnung auf Market** oder wird **storniert**. Ein Stop-and-Reverse bleibt ein Market-Flip.
- **Niedrigeren Timeframe für SL/TP prüfen**: Erreicht eine Candle sowohl Stop als auch Ziel oder füllt ein Limit mitten in der Candle, liest der Lauf einen niedrigeren Timeframe desselben Instruments (gleicher Anbieter) *nur innerhalb dieser Candle*, um zu sehen, was zuerst kam. Er liest zuerst einen gespeicherten Datensatz bei diesem Timeframe, dann von einem früheren Lauf heruntergeladene Candles, die für spätere Läufe aufbewahrt werden. *Auto* nimmt den feinsten gespeicherten Timeframe, sonst den feinsten, den der Anbieter in einer Anfrage pro Candle liefert. Candles des niedrigeren Timeframes, die nicht zur Candle passen (andere Extrema), werden nicht genutzt.
- **Fehlende Candles des niedrigeren Timeframes herunterladen**: Mit dieser Option werden die Candles, die ein Lauf braucht und nicht hält, vom Anbieter heruntergeladen, und nur diese. Nach einem Lauf sagt ein Hinweis, wie viele Candles mangels dieser als Stop entschieden wurden, mit den Kosten (Anfragen und Zeit) und einem Kästchen zum Abrufen. Ein Download bis etwa 4 Minuten startet von selbst in einem Fortschrittsfenster; ein längerer wartet auf dich, mit dem Rate-Limit des Anbieters und dem auf seinem Konnektor verbleibenden Kontingent, da er scheitern kann, wenn nicht genug übrig ist. Der Lauf wird dann mit den heruntergeladenen Candles wiederholt. Lehnt der Anbieter ab (keine Berechtigung oder kein Abo für diese Daten, ein Schlüssel, ein Rate-Limit) oder scheitert dreimal in Folge, hört der Lauf auf, ihn zu fragen, und sagt das neben den Ergebnissen. Eine Paper-Sitzung mit der Option lädt die Candles unter der gerade geschlossenen Candle herunter, wenn sie sie braucht, und wartet einen Moment, bis der Anbieter sie veröffentlicht; ohne die Option oder wenn sie nie kommen, gewinnt der Stop.
- **Bid-/Ask-Preise nutzen**: Fills lesen Bid und Ask statt Mitte und Spread (ein Market-Kauf zahlt den Ask, Stop und Ziel eines Longs lösen am Bid aus). Sie werden vom Anbieter nur für die Candles geholt, in denen ein Fill stattfinden kann, auf dem einfachsten Weg, den er bietet (siehe Tabelle unten), und für spätere Läufe aufbewahrt. Ein kurzer Abruf startet nach dem Lauf von selbst, ein langer fragt vorher, mit demselben Fortschrittsfenster wie beim niedrigeren Timeframe. Eine Candle ohne Bid/Ask nutzt den Spread, und das Ergebnis sagt, wie viele das taten. Anbieter ohne historisches Bid/Ask lassen die Option aus und sagen warum.
- **Separate Maker-Gebühr** (im Block Kosten): Limit-Fills (Einstiegs- und Ausstiegs-Limits, Take-Profit) zahlen ihre eigene Gebühr, negativ für einen Rebate.
- **Rohpreise**: Aktien und ETFs werden auf Bars bepreist, die um Splits und Dividenden bereinigt sind, wo der Anbieter sie liefert (Yahoo, EODHD, Alpaca; Bars von Interactive Brokers kommen splitbereinigt). Hake dies an, um auf gehandelten Preisen zu laufen. Ein Aktien-Datensatz ohne bereinigte Serie wird neben den Ergebnissen markiert.

Das Ergebnis zeigt platzierte, gefüllte und verfallene Limit-Orders, bei wie vielen Candles sowohl ein Stop als auch ein Ziel in Reichweite waren und wie sie entschieden wurden (niedrigerer Timeframe oder Worst Case), und wie viele Fills auf Bid/Ask bepreist wurden.

#### Bid/Ask und Live-Preis je Anbieter {#bid-ask-providers}

| Anbieter | Historisches Bid/Ask (Backtest) | Live-Preis (Paper-Stops, -Ziele, -Limits) | Live-Bid/Ask (Paper-Fills) |
|---|---|---|---|
| Capital.com | Bid- und Ask-Candles | Ja | Ja |
| OANDA | Bid- und Ask-Candles | Nein, bei Candle-Schluss geprüft | Nicht genutzt |
| Interactive Brokers | Bid- und Ask-Candle-Serien | Ja | Ja |
| FOREX.com | Bid- und Ask-Candles | Nein, bei Candle-Schluss geprüft | Nicht genutzt |
| Alpaca | Quotes bei Eröffnung und Schluss der Candle (Aktien, ETFs, Krypto) | Ja | Ja |
| Massive | Quotes bei Eröffnung und Schluss der Candle (Aktien, ETFs, Optionen, Forex) | Ja | Ja |
| Binance, Binance Futures, Kraken, OKX, Bitget, Coinbase | Keine, der Spread gilt | Ja | Ja |
| TradeStation, Yahoo, EODHD, Alpha Vantage | Keine, der Spread gilt | Nein, bei Candle-Schluss geprüft | Nicht genutzt |

Bei Quotes, die zur Eröffnung und zum Schluss gelesen werden, ist der Spread innerhalb der Candle ihr Durchschnitt um das eigene High und Low der Candle.

#### Paper Trading auf dem Live-Preis {#paper-live}

Eine Paper-Sitzung liest ihre Signale auf geschlossenen Candles, in ihrem Timeframe, und beobachtet ihre Level dazwischen auf dem Live-Preis:

- **Stop-Loss, Take-Profit und Limit-Orders** (Einstieg und Ausstieg) lösen beim ersten Live-Preis aus, der sie erreicht, ohne auf den Schluss der Candle zu warten. Der **Trailing-Stop** bewegt sich weiterhin bei jedem Schluss, und der Live-Preis löst ihn auf dem dann gesetzten Level aus.
- Der Fill wird auf dem Live-Bid/Ask bepreist, wenn die Strategie Bid/Ask nutzt, sonst auf dem Live-Preis mit Spread und Slippage aus den Einstellungen. Er wird **sofort gemeldet**, und der nächste Lauf wiederholt ihn auf seiner Candle, wie er geschah, ohne zweiten Alarm.
- Ein Anbieter ohne Live-Preis für das Instrument wird im Sitzungsdialog benannt: Dort werden Stops, Ziele und Limits bei Candle-Schluss geprüft. Stoppt der Live-Feed oder ist die App offline, übernimmt wieder die Candle, und das Sitzungslog sagt das.
- Ein Einstieg wird **beim Schluss gemeldet, der sein Signal gibt**. Eine Market-Order wird zum Preis dieses Schlusses angekündigt, mit ihrer Größe (auf dem zu diesem Schluss bewerteten Eigenkapital genommen) und ihrem **Betrag** an Geld für einen Broker, der eine Notional-Order in Bruchteilseinheiten nimmt, und füllt dann zur nächsten Eröffnung: beim ersten Live-Preis, wenn der Anbieter einen streamt (ihr Stop und Ziel werden dann ab diesem Fill live beobachtet), sonst zur Eröffnung dieser Candle, sobald sie geschlossen hat. Der Preis wird ohne zweiten Alarm aktualisiert. Eine Limit-Order wird beim Platzieren angekündigt (*Triggering limit order*), und ihr Fill wird gemeldet, wenn er geschieht.
- Ein live gefüllter Limit-Einstieg erhält seinen Stop und sein Ziel vom nächsten Lauf; bis dahin ist diese Position bei Candle-Schluss geschützt.

Paper Trading läuft auf derselben Engine wie der Backtest, mit denselben Optionen: Bei aktivem *Fehlende Candles des niedrigeren Timeframes herunterladen* oder Bid/Ask holt ein Lauf, was die gerade geschlossene Candle braucht, und wartet einen Moment, bis der Anbieter sie veröffentlicht. Die Sitzungszeile listet die Limit-Orders, die sie im Markt hat.

### Strategien und eigene Indikatoren {#strategies-and-custom-indicators}

- **Benannte Strategien**: vollständige Strategiekonfigurationen speichern, suchen, duplizieren und bearbeiten.
- **Strategie-Versionen** (optional): Ist die Versionierung unter [Einstellungen → Versionen](/de/config/settings#versioning) an, kann eine gespeicherte Strategie über das Verlaufsmenü in der Kopfzeile versioniert werden. Speichere eine Version (datiert, mit optionaler Notiz), sieh nach, was jeder Schritt sagte, stelle sie wieder her (der wiederhergestellte Zustand wird als eine neue Version gespeichert, mit dem Datum der wiederhergestellten Version vermerkt, und behält seinen Namen) oder lösche sie. Der Verlauf öffnet sich in einem Fenster mit einer Suche über Notizen und Daten; die Version, die der aktuellen Strategie entspricht, ist markiert. Notizen sind auf 500 Zeichen begrenzt. Nicht gespeicherte Änderungen werden gespeichert, bevor die Version erstellt wird. Beim Ausschalten der Versionierung für eine Strategie wirst du gefragt, ob ihre Versionen behalten oder gelöscht werden sollen. Das Löschen einer Strategie mit Versionen fragt, ob sie behalten werden sollen; behaltene können sie unter **Gelöschte Strategien mit Versionen** im Tab Strategien wiederherstellen. Agenten mit Schreibzugriff auf Backtest können dasselbe über [MCP](/de/config/ai-agents), mit einer Version nach jedem Update wie bei einem Commit.
- **Eigene Indikatoren**: baue deine eigenen aus benannten Schritten, ohne Code. Jeder Schritt wendet entweder einen eingebauten Indikator auf eine **Quelle** an (ein Preisfeld oder die Ausgabe eines früheren Schritts) oder berechnet eine **Formel**, die frühere Schritte beim Namen nennt (`@volume / SMA(@volume)`, mit `+ − × ÷`, `min`, `max`, `abs`, `clamp`). So kannst du Indikatoren verketten: eine Hull MA eines RSI, ein MACD eines RSI, ein geglättetes Volumenverhältnis und so weiter. Indikatoren, die ganze Candles lesen (ATR, Stochastic, ADX, VWAP…), gelten nur für den Preis, nicht für einen abgeleiteten Schritt. Die hervorgehobenen Schritte sind die Ausgabe. Eigene Indikatoren werden im Regel-Editor neben den eingebauten zu Operanden, und die Bibliothek wird **mit dem Chart geteilt**, der dieselbe Definition zeichnet ([Eigene Indikatoren im Chart](#custom-indicators-on-the-chart)).
- **Durchsuchbare Indikatorauswahl**: Wähle Indikatoren aus einer gruppierten Liste mit Tipp-zum-Filtern (im Regel-Editor und im Builder für eigene Indikatoren), statt ein langes Dropdown zu scrollen.

### Datumsfenster und Parameter-Sweeps (API)

Zwei Fähigkeiten leben in der API statt im Formular. Sie existieren für den [Assistenten](/de/modules/agent) und für alle, die die App über [MCP](/de/config/ai-agents) steuern:

- **Läufe mit Datumsfenster.** `from` / `to` bei einem Lauf (und bei der Ausrichtungsvorschau) beschränken die simulierte Spanne, was Walk-Forward-Validierung und Regime-Slicing brauchen: 2019 bis 2021 ausführen, dann 2022 bis 2024, und vergleichen. `to` schließt den ganzen Tag ein.
- **`POST /api/backtest/sweep`**: ein Parameter-Raster serverseitig ausführen und **jeden Trial zurückbekommen**, zusammen mit der Trial-Anzahl. Raster-Pfade reichen in Arrays (`long.entry.conditions.0.left.period`), Indikatorperioden sind also sweepbar; auf 4 Achsen und 64 Trials begrenzt.

Ein Sweep liefert außerdem einen **Deflated Sharpe**: den Sharpe, den der Beste von N *wertlosen* Strategien erwartungsgemäß erreichen würde, angesichts dessen, wie stark diese konkreten Trials variierten. Vergleiche den Gewinner mit dieser Latte, nicht mit null: Bei echten Tagesbars heißt der Beste von acht Moving-Average-Crossovers mit 0,59 gegenüber einer Selektionslatte von 0,70 *kein Beleg für einen Edge*, was das Maximum allein verborgen hätte.

Er liefert auch die **Wahrscheinlichkeit des Backtest-Overfittings** (PBO) des Rasters: Die Equity-Kurven der Trials werden in 16 Blöcke geschnitten, jede Hälfte wird einmal als In-Sample- und einmal als Out-of-Sample-Menge genutzt, und PBO ist der Anteil der Splits, in denen der In-Sample-Gewinner Out-of-Sample in der unteren Hälfte landet. Der Quant-Tab [Vergleichen](#quant) berechnet dieselbe Zahl auf gespeicherten Läufen.

### Optimizer

Nimm einen fertigen Lauf und **variiere seine Parameter**: Indikatorlängen und Schwellen pro Seite, Stops, Sizing, Kosten, Portfolio-Limits, Grid-Einstellungen und welche Wochentage ausgeschlossen werden (jede Teilmenge wird probiert). Jeder Parameter bekommt ein von / bis / Schritt, und die Kopfzeile zählt die Varianten, während du sie weitest, gedeckelt, damit ein Raster endlich bleibt.

- **Vor dem Start** schätzt er die Kosten aus dem, was frühere Läufe auf deiner Maschine gemessen haben: Millisekunden pro Variante, Worker, Gesamtzeit. Du kannst jederzeit stoppen und behalten, was berechnet wurde.
- **Ranking** nach der gewählten Kennzahl (Sharpe, Sortino, Rendite, Profit Factor, Trefferquote, Expectancy, max. Drawdown, Trades); jede Spalte sortiert danach neu. Mit einem Out-of-Sample-Split ist jede gerankte Zahl die **In-Sample**-Zahl, und die Out-of-Sample-Rendite wird gezeigt, aber nie gerankt: Ein darauf gewählter Gewinner hätte sie gesehen.
- **Analyse** zeigt die Streuung der gewählten Kennzahl über jede Variante: schlechteste, durchschnittliche, beste und wie viele positiv ausfielen. Eine einzelne gute Zahl bedeutet wenig, wenn ihre Nachbarn schrecklich sind.
- **Multiple-Testing-Abschlag**: gemessen an den Candles pro Jahr, die die Daten tatsächlich haben (ein 24/7-Markt hat etwa fünfmal mehr Stunden-Candles als eine Aktie), wird der beste Sharpe der **Selektionslatte** gegenübergestellt, dem Sharpe, den der Beste von so vielen *wertlosen* Strategien erwartungsgemäß erreichen würde. Unter der Latte genügt das Probieren so vieler Varianten, um den Gewinner zu erklären.
- Klicke eine Variante an, um ihren **vollständigen Backtest** zu lesen, aus ihren eigenen Einstellungen wiederholt. Nichts wird gespeichert, bis du sie im Verlauf **behältst**.

### Out-of-Sample-Split

Teile die Daten in einen **In-Sample**-Kopf und einen **Out-of-Sample**-Schwanz; die Strategie läuft auf beiden, und die beiden Statistikblöcke (Rendite, Profit Factor, Trefferquote, max. Drawdown, Trades) werden nebeneinander gezeigt. Eine große Lücke zwischen den Spalten ist ein Zeichen für Overfitting.

### Paper Trading

*Ein fertiger Lauf, der vorwärts weiterläuft.* Drücke **Paper Trade** bei einem Ergebnis, und die Strategie handelt weiter auf Papier, nach Zeitplan, und meldet an die von dir gewählten Kanäle. Es gibt keine zweite Engine: Jeder Lauf simuliert das Fenster mit dem gewöhnlichen Backtest neu und meldet, was sich geändert hat, ein Paper-Fill ist also konstruktionsbedingt der Fill, den der Backtest für dieselben Candles gezeigt hätte.

- **Die Strategie ist eingefroren**, wie sie lief, Instrumente eingeschlossen. Das spätere Bearbeiten dieser Strategie ändert eine laufende Sitzung nicht; das eigene Formular der Sitzung bietet an, sie zu aktualisieren oder eine Kopie zu starten, wenn du die Strategie erneut speicherst.
- **Der erste Lauf füllt das Buch.** Jeder Round Trip, der bereits im Fenster liegt, wird sofort erfasst, in einem einzigen Ereignis zusammengefasst: Das Öffnen einer Sitzung feuert keinen Schwall an Alarmen über die Historie. Nur was danach geschieht, ist ein Fill, der eine Nachricht wert ist.
- **Fenster**: wie viel Historie jeder Lauf der Engine zuführt (nachlaufende Candles oder ein fester Start).
- Der Tab **Paper** listet die Sitzungen mit Status, nächstem Lauf, offenen Positionen und ungelesenen Ereignissen, und jede kann jetzt ausgeführt, pausiert, fortgesetzt oder gelöscht werden. Das Ereignislog wird aufbewahrt, ob etwas gesendet wurde oder nicht, sodass das, was nicht zugestellt werden konnte, noch da ist, wenn du zurückkommst.

#### Zeitplan

Jeder Lauf und seine Daten gehen nach der eigenen Uhr der Candle.

- **Intervall** (alle N Minuten), **täglich**, **wöchentlich**, **monatlich** oder **einmalig**, in einer echten Zeitzone.
- Ein Intervall feuert auf dem **Candle-Raster**, nie in der Sekunde, in der die Sitzung angelegt wurde: jede Minute bei :00, alle 15 Minuten bei :00 / :15 / :30 / :45, stündlich zur vollen Stunde.
- Der Lauf **lädt die Candle herunter, auf die er wartet**, in die eigenen Datensätze der Sitzung. Die laufende Candle wird nie gespeichert: Gelesen wird die letzte *geschlossene*, eine Woche von ihrem Montag bis zum nächsten, und ein Tag erst, wenn seit seinem Stempel ein voller Tag vergangen ist (eine US-Tageskerze landet nach 04:00 UTC). Eine letzte Candle, die vor Ende ihrer Periode gespeichert wurde, wird erneut gelesen. Eine Candle, die der Anbieter noch nicht veröffentlicht hat, wird über ein paar Sekunden erneut angefragt, und eine Periode ganz ohne Handel schreibt nichts, was schlicht heißt, dass der nächste Lauf nichts Neues zu simulieren hat.
- Ein Lauf, der keine neue Candle findet, kostet nichts: Er simuliert gar nicht.

#### Was gesendet wird, und wohin

- **Benachrichtigen**: bei jedem Lauf, nur bei einem neuen Fill oder nie. *Jeder Lauf* meldet auch die ruhigen, und das ist der einzige Weg, „nichts ist passiert“ von „die Engine hat aufgehört zu laufen“ zu unterscheiden.
- **Nachrichten gruppieren**: sofort senden oder ein stündlicher, täglicher oder wöchentlicher Digest. Ein gruppierter Versand ist eine Nachricht über einen Zeitraum, kein Ping pro Fill.
- **Alarm bei** Einstiegen, Ausstiegen oder beidem, und optional nur für die **Instrumente**, die du nennst.
- **Kanäle**: die [Benachrichtigungskanäle](/de/config/settings#notifications), die Backtest gewährt wurden, alle oder die von dir gewählten. Ohne eingerichteten Kanal wird nichts gesendet, und jedes Ereignis wird trotzdem in der App erfasst.

#### Eigene Nachrichten

Drei Nachrichten, drei Formulierungen, jede mit eigenem Vokabular: **Einstieg**, **Ausstieg** und **Zusammenfassung** (die gruppierte). Lass ein Feld leer, und die eingebaute Formulierung wird genutzt.

- Jede hat einen **Titel** und einen **Text**, mit Platzhaltern geschrieben:

```
{{trade.ticker}} {{trade.direction}} at {{trade.entry_price}}
```

- **Variablen** listet genau auf, was diese Nachricht lesen kann, mit einem Beispielwert; klicke eine an, um sie einzufügen. Ein Einstieg liest die Einstiegshälfte des Trades, ein Ausstieg das Ganze (P&L eingeschlossen), und die Zusammenfassung liest den Zeitraum, `since.*` (seit dem letzten Alarm), `total.*` (seit Sitzungsbeginn) und `open.*` (was gerade gehalten wird), plus `session.*`, `event.*` und `stats.*`. Ein Pfad, den eine Nachricht nicht lesen kann, wird ihr nicht angeboten.
- **Filter** werden `| name:arg` geschrieben, und `upper`, `lower`, `trim` und `json` nehmen keines:

```
{{trade.pnl | round:2}} {{since.from | date:YYYY-MM-DD}} {{trade.exit_reason | default:-}}
```

- Die **Vorschau** ist der eigene Renderer des Servers, was sie zeigt, ist also das, was gesendet würde. Sie läuft auf einem Beispiel-Trade und den letzten Zahlen der Sitzung, ein Wert, den die Sitzung noch nicht erzeugt hat, ist daher an Ort und Stelle in Klammern markiert, und der Rest der Nachricht wird weiter gerendert.

### Ergebnisse

- Kennzahlen: Rendite (gegenüber **Buy & Hold**), Netto-PnL und Gebühren, Trefferquote, Profit Factor, Expectancy, max. Drawdown, Sharpe/Sortino.
- **Equity-Kurve** über dem Preis mit Einstiegs-/Ausstiegsmarkierungen.
- Eine vollständige **Performance-Zusammenfassung** (Bruttogewinn/-verlust, Payoff Ratio, größter Gewinn/Verlust, max. aufeinanderfolgende Gewinne/Verluste, durchschnittliche Bars im Trade…) und die vollständige **Trade-Liste** mit **MAE/MFE** pro Trade (schlechtester offener Verlust / bester offener Gewinn im Trade), filterbar, und Ausstiegsgründen (Signal verblasst, Ausstiegssignal, Stop-Loss, Take-Profit, umgekehrt, Datenende).
- **Läufe speichern** unter einem Namen und einen Verlauf behalten, um Strategien später zu vergleichen. Das Menü **Berichte** eines fertigen Laufs exportiert ihn vollständig, pro Seite und pro Asset: ein **PDF** mit jedem Chart oder **Markdown** nur mit den Zahlen. Keines trägt die Trade-Liste; die Datei wird nach der Strategie und dem Zeitpunkt des Exports benannt.

## Quant Tools {#quant}

Analysen auf deinen Datensätzen, deinen gespeicherten Backtests und, bei Derivaten, direkt von einem Anbieter. Die Tabs kommen in sechs Gruppen.

Welche Tabs was brauchen:

- **Asset** und **Multi-Asset** lesen gespeicherte [Historical Data](#histdata)-Datensätze.
- **Strategie** liest gespeicherte [Backtest](#backtest)-Läufe.
- **Sizing** und **Rechner** nehmen Zahlen, die du tippst (Vol Targeting liest zusätzlich einen Datensatz).
- **Derivate** liest einen Interactive-Brokers- oder Massive-[Konnektor](/de/config/connectors), der Quant gewährt ist.

### Asset

Ein Datensatz und ein Zeitfenster, von jedem Tab der Gruppe geteilt.

- **Risiko**: annualisierte historische Volatilität, max. Drawdown, **Value at Risk** und **Conditional VaR** bei deinem Konfidenzniveau. Der Tail jenseits des VaR wird auf drei Arten geschätzt (Normal, Cornish-Fisher, ein Generalized-Pareto-Fit der schlechtesten Verluste). Eine Tabelle listet die schlimmsten Drawdowns mit Tiefe, Daten und den Bars, die bis zum Tief und bis zur Erholung brauchten, denn eine einzelne Max-Drawdown-Zahl verbirgt, wie lange das Loch zum Füllen brauchte.
- **Statistik**: was für eine Art Serie das ist.
  - Verteilung gegenüber einer Normalverteilung mit gleichem Mittelwert und gleicher Volatilität: Schiefe, Excess-Kurtosis, ein QQ-Plot.
  - Serielle Abhängigkeit: Autokorrelation der Renditen und der absoluten Renditen, mit Ljung-Box-p-Werten. Renditen zeigen selten eine, absolute Renditen meist: Das ist Volatilitäts-Clustering.
  - Trend oder Mean Reversion: Hurst (R/S und DFA), Varianzverhältnisse nach Horizont und die Preis-Halbwertszeit.
  - Stationarität: ADF und KPSS, zusammen gelesen.
  - Ob sich die Sharpe Ratio vom Zufall unterscheiden lässt: t-Statistik, Probabilistic Sharpe, minimale Track-Record-Länge.
- **Volatilität**:
  - Fünf Schätzer auf denselben Bars. Close-to-Close nutzt nur die Schlusskurse; Parkinson, Garman-Klass, Rogers-Satchell und Yang-Zhang lesen auch die Spanne der Bar.
  - Ihre gleitenden Verläufe.
  - Ein **Volatilitätskegel**, der sagt, ob der heutige Wert für seinen Horizont hoch oder niedrig ist.
  - Eine **GARCH(1,1)**-Prognose mit ihrer Persistenz und der Halbwertszeit von Schocks.
- **Regime**:
  - Ein Gaußsches Hidden-Markov-Modell teilt die Renditen in 2 bis 4 Zustände, den ruhigsten zuerst, und schattiert den Preis-Chart nach dem wahrscheinlichsten Zustand.
  - Rendite, Volatilität, verbrachte Zeit und typische Verweildauer jedes Zustands.
  - Die Übergangswahrscheinlichkeiten und in welchem Zustand der Markt jetzt am wahrscheinlichsten ist.
- **Ereignisse**: wähle eine Bedingung (Gap, großer Schluss, SMA- oder RSI-Kreuzung, neues N-Bar-Hoch oder -Tief, Serie, Volumenspitze) und sieh, was der Markt danach tat.
  - Forward-Renditen bei mehreren Horizonten gegenüber der unbedingten Basislinie über dieselben Bars, mit einem p-Wert pro Horizont.
  - Der durchschnittliche Verlauf rund um das Ereignis.
- **Saisonalität**:
  - Eine Heatmap Monat × Wochentag für mittlere Rendite, Volatilität, Bar-Spanne oder Volumen, plus Streifen nach Monat, Wochentag und (nur Intraday) Stunde.
  - Jede Zelle zeigt ihre Stichprobenzahl und Trefferquote. Die Stundenuhr ist **UTC**.

### Multi-Asset

Zwei oder mehr Datensätze mit demselben Timeframe.

- **Portfolio**: Korrelationsmatrix, **Efficient Frontier** (eine Wolke zufälliger Allokationen; klicke den Max-Sharpe- oder Min-Volatilitäts-Punkt, um seine Gewichte zu lesen) und **Risk Parity**.
- **Paare**:
  - Kointegration (Engle-Granger und Johansen in beide Richtungen) und der Spread mit Hedge-Ratio, Z-Score und Halbwertszeit.
  - Gleitende Korrelation und Beta.
  - Lead-Lag-Korrelation und Granger-Kausalität, um zu sehen, ob eine Serie zuerst zieht.
- **Basket**:
  - **PCA**: wie viele unabhängige Wetten der Basket wirklich hält.
  - Ein Korrelations-**Dendrogramm**: wer sich gemeinsam bewegt.
  - Eine **Hierarchical-Risk-Parity**-Allokation.
  - Eine Relative-Stärke-Tabelle über 1, 3, 6 und 12 Monate.
  - Ein Stresstest, der eine Gewichtung durch jede vergangene Krise hält, die die Daten abdecken (2008, 2020, 2022 und andere).
- **Regression**:
  - Die Renditen eines Assets auf einen oder mehrere Faktor-Datensätze (einen Index, Anleihen, Gold, einen Sektor).
  - Alpha mit seiner t-Statistik, Beta jedes Faktors, R², Tracking Error und Information Ratio.
  - Up-/Down-Capture und ein gleitendes Beta.

Asset-Klassen zu mischen ist in Ordnung, auch verschiedene Anbieter: Bars werden nach der **Periode** zugeordnet, zu der sie gehören, nicht nach dem Zeitstempel, den der Anbieter ihnen gab. Eine tägliche Krypto-Candle öffnet um 00:00 UTC und eine US-Aktien-Candle zum Beginn der New Yorker Sitzung, und beide sind derselbe Tag. Zwei Dinge folgen daraus, und das Panel sagt, welches galt:

- Ein Basket, der einen 24/7-Markt mit einem mit Börsenzeiten mischt, wird **wöchentlich** gemessen. Täglich ausgerichtet würde die Wochenendbewegung des durchgehenden Assets auf dieselbe Zeile wie der Montag des anderen fallen und unterschätzen, wie stark sie sich wirklich gemeinsam bewegen.
- Die Annualisierung wird **an der Uhr gezählt**, nicht angenommen: Dieselben Tages-Datensätze sind 252 Perioden im Jahr an einer Börse und 365 in einem 24/7-Markt.

Intraday-Datensätze sind die Ausnahme: 4h-Bars, die an einer Handelssitzung verankert sind, und 4h-Bars, die an der Uhr verankert sind, liegen 90 Minuten auseinander, daher wird ein Intraday-Basket mit gemischten Anbietern abgelehnt, statt angenähert. Nutze Tages-Datensätze oder einen Anbieter für den ganzen Basket.

### Strategie

Gespeicherte Backtest-Läufe. Jeder Lauf wird auf dem Server wiederholt, um seine exakten Trades neu zu erzeugen.

- **Monte Carlo**: Trades eines Laufs tausendfach neu ziehen (einzeln oder in Blöcken, um Serien beisammen zu halten).
  - **Perzentilbänder** auf dem Equity-Pfad, dem Endkapital und dem max. Drawdown.
  - Die Wahrscheinlichkeit, mit Verlust zu enden.
  - Ein **Ruinrisiko**: der Anteil der Pfade, deren Eigenkapital je auf eine von dir gesetzte Schwelle fiel.
  - Die echte Equity-Kurve obendrauf gezeichnet.
- **Trades**: was die Trades pro Risikoeinheit wert sind.
  - Expectancy in Währung und in **R**, die R-Multiple-Verteilung und **SQN** (auf höchstens 100 Trades, mit Van Tharps Einstufung).
  - Das **MAE/MFE**-Streudiagramm: wie viel Hitze die Gewinner abbekamen, wie weit die Verlierer zuerst liefen.
  - 1R ist der Stop, wenn der Lauf einen prozentualen Stop hat, sonst der durchschnittliche Verlust, und die Seite sagt welcher. Grid- und DCA-Läufe erfassen kein MAE/MFE, das Streudiagramm entfällt dafür also.
- **Vergleichen**: 2 bis 20 Läufe auf ihren gemeinsamen Daten.
  - Rebasierte Equity-Kurven, eine Tabelle mit Rendite, Volatilität, Sharpe und Drawdown, und die Korrelation ihrer Renditen.
  - Der **Deflated Sharpe** des besten Laufs, wobei die anderen als die Trials zählen, aus denen er gewählt wurde.
  - Die **Wahrscheinlichkeit des Backtest-Overfittings** (PBO, per kombinatorisch symmetrischer Kreuzvalidierung über 8 bis 16 Blöcke). Ein PBO über 50 % heißt, dass der In-Sample-Gewinner Out-of-Sample meist in der unteren Hälfte landet.

### Sizing

- **Positionsgröße**: aus deinem Stack, Einstieg, Stop und Risiko (Prozent oder fest) die **Größe, das Notional, die Margin, das Exposure und das Reward:Risk**. Sie kann **Stops vorschlagen** aus einem Datensatz (Volatilität, ATR, Swing) und den Einstieg aus dem letzten Schluss füllen.
- **Kelly**: der Kelly-Bruchteil aus Trefferquote und Payoff, mit halbem und Viertel-Kelly. Volles Kelly maximiert das langfristige Wachstum, schwankt aber stark; die meisten Trader dimensionieren mit halbem oder Viertel-Kelly.
- **Vol Targeting**:
  - Halte Ziel-Volatilität ÷ geschätzte Volatilität des Assets, wobei die Schätzung gleitend oder EWMA ist, auf einen maximalen Hebel gedeckelt.
  - Jede Bar wird auf der Schätzung dimensioniert, die vor ihr bekannt war, es gibt also keinen Look-ahead.
  - Zeigt das Gewicht und die jetzt zu haltenden Einheiten für dein Eigenkapital, und den skalierten Verlauf gegenüber dem flachen Halten des Assets.
- **Ruinrisiko**: die Chance, dass eine Trefferquote, ein Payoff und ein Risiko pro Trade einen gegebenen Drawdown erreichen, bei festem Betrag oder festem Bruchteil, der pro Trade riskiert wird. Drei Antworten:
  - Die geschlossenen Formen: Vince, für einen festen Betrag; die Cramér-Lundberg-Schranke, für beide Sizing-Arten.
  - Eine Simulation über die von dir gesetzte Zahl an Trades, mit ihrem Standardfehler und der Kurve der Ruin-Wahrscheinlichkeit nach Trade-Anzahl.

### Rechner

- **Optionen**: Black-Scholes-Merton-Preis und Greeks (Vega und Rho pro Punkt, Theta pro Tag), ein Binomialbaum für amerikanische Ausübung mit der Prämie für vorzeitige Ausübung, und die **implizite Volatilität** eines notierten Preises.
- **Futures-Basis**: aus einem Spot- und einem Futures-Preis die Basis, das daraus folgende Carry pro Jahr, den impliziten Repo, den fairen Wert bei deinem Zins und deiner Rendite und die Roll-Rendite zum nächsten Kontrakt.
- **Zinseszins**:
  - Wohin ein Kapital und eine Rendite pro Periode führen, mit Einzahlungen.
  - Die nötige Rendite, um ein Ziel zu erreichen, und wie viele Perioden es dauert.
  - Der Gewinn, den es braucht, um aus einem Drawdown zurückzuklettern.
- **Sharpe-Test**: für einen Sharpe, der ohne seine Daten genannt wird.
  - Lässt er sich von null oder von einem Benchmark unterscheiden?
  - Wie lange einen Track Record braucht er?
  - Was bleibt davon, wenn man die Zahl der probierten Strategien einrechnet (Deflated Sharpe, mit Schiefe und Kurtosis)?

### Derivate

Diese lesen den Anbieter direkt statt eines gespeicherten Datensatzes und brauchen daher einen **Interactive-Brokers**- oder **Massive**-Konnektor, der Quant gewährt ist.

Ein Lauf sind Dutzende Anbieteranfragen, vom Anbieter getaktet. Die Seite zeigt die erledigten Anfragen von den geplanten und was gerade geholt wird. Das Geholte wird sechs Stunden aufbewahrt, sodass das Ändern eines Zinses oder einer Roll-Regel neu rechnet, ohne den Anbieter erneut zu fragen.

- **Futures-Kurve**: die Kontrakte mit Fälligkeit eines Produkts (`ES@CME` bei Interactive Brokers, `ES` bei Massive), abgelaufene eingeschlossen.
  - Die **Terminstruktur** der letzten abgeschlossenen Sitzung.
  - Die **Roll-Rendite** zwischen dem Front- und dem nächsten Kontrakt über die Zeit.
  - Eine fortlaufende Serie aus Halten des Fronts und Rollen, per Verhältnis rückwärts bereinigt, sodass sie auf dem heutigen Preis endet, neben der zusammengespleißten, die bei jedem Roll springt.
  - Das Rollen ist eine Kalenderregel: Der Front ist der nächste Kontrakt mit mehr als *N* verbleibenden Tagen.
  - Mit einem Spot-Ticker (zum Beispiel `SPX` als Index) kommen die Basis, das Carry `ln(F/S)` pro Jahr, der implizite Repo und die Fehlbewertung gegenüber dem fairen Wert bei dem von dir eingegebenen Zins und der Rendite hinzu.
  - Massive-Kurven nutzen den Settlement-Preis jeder Sitzung.
- **IV-Fläche**: die Optionskette eines Basiswerts.
  - Einige Verfälle, zwischen der von dir gesetzten Mindest- und Höchsttageszahl verteilt, Out-of-the-Money-Strikes auf jeder Seite, jeder Preis in eine implizite Black-Scholes-Merton-Volatilität verwandelt.
  - Smiles nach Verfall, die **ATM-Terminstruktur**, 25-Delta-**Risk-Reversal** und -**Butterfly** und eine Prüfung, dass die ATM-Gesamtvarianz von einem Verfall zum nächsten nie fällt.
  - Interactive Brokers bepreist jede Option zum letzten Stunden-Mittelpunkt, in derselben Stunde wie der Basiswert genommen, sodass kein Optionsmarktdaten-Abo nötig ist. Massive bepreist jede zum Sitzungsschluss.
  - Die Seite nennt dir die Zahl der Anfragen vor dem Start: Mit einem kostenlosen Massive-Schlüssel (5 pro Minute) braucht eine Kette mehrere Minuten.
- **Implizit vs. realisiert** (nur Interactive Brokers): die 30-Tage-implizite Volatilität des Basiswerts, bis zu zehn Jahre zurück, gegenüber der Close-to-Close-Volatilität vor und nach jedem Tag.
  - Die **Volatilitätsprämie**: IV minus die Volatilität, die folgte.
  - **IV-Rang** und **Perzentil** über einen von dir gewählten Rückblick.
  - Wie gut IV die realisierte Volatilität prognostizierte (eine Regression der folgenden realisierten auf IV).
  - Die Korrelation von IV-Änderungen mit Preisbewegungen.
