# Trading Journal

Erfasse jeden Trade, in jeder Währung, und erhalte ehrliche Performance-Statistiken: Equity-Kurve, Trefferquote, Expectancy, Profit Factor, Drawdown, Sharpe und mehr. Das Journal ist in elf Tabs gegliedert: **Aufschlüsselung**, **Analysen**, **PnL-Kalender**, **Trades**, **Was-wäre-wenn-Labor**, **Strategien & Kapital**, **Tags**, **Vorlagen**, **Gebühren & Währung**, **Import**, **Offene Aufgaben**.

## Kategorien

Trades leben in **Kategorien**: Ordner wie *Krypto-Scalping* oder *Langfristige Aktien*, jeder mit eigener Farbe, eigenem Kapital und eigenen Statistiken. Lege sie in der Kategorieleiste an; per Ziehen umsortieren. Das Löschen einer Kategorie löscht ihre Trades. Die **Standard**-Kategorie lässt sich nicht löschen.

## Kapital einrichten

Gib unter **Strategien & Kapital** jeder Kategorie einen **Anfangsbestand** und erfasse über die Zeit **Nachschüsse** und **Entnahmen**. Daran werden Rendite, Equity-Kurve und Drawdown gemessen; ohne ihn bekommst du weiterhin PnL, aber keine Renditen.

Du kannst auch **Strategien** mit ihren Signalnamen benennen (z. B. *Breakout, Pullback*). Versieh Trades mit einer Strategie/einem Signal, und der Tab Aufschlüsselung kann danach filtern: So findest du heraus, welche Setups sich tatsächlich auszahlen.

## Vorlagen

Vorlagen steuern das Trade-Formular. Eine vorgefertigte **Standard-Trade**-Vorlage existiert; erstelle eigene pro Markt oder Stil:

- **Reservierte Felder** (Seite, Preise, Menge, Gebühren, Hebel, Multiplikator, Währung, Einheitentyp…) speisen die Performance-Statistiken.
- **Eigene Felder** (Text, Zahlen, Auswahllisten…) sind frei: Setup-Note, Marktbedingung, was immer du verfolgst.
- Eine Vorlage kann ein **Standard-Gebührenmodell** festlegen, das beim Erfassen daraus vorausgewählt ist (pro Trade überschreibbar).

## Trades erfassen

Wähle im Tab Trades eine Vorlage (oder die *Quick*-Vorlage, die alle Felder zeigt) und fülle das Formular aus. Zwei Stufen:

- **Einfach**: ein Einstieg, ein Ausstieg (oder den Ausstieg für eine offene Position leer lassen).
- **Erweitert**: Auf- und Abstocken mit mehreren **Einstiegs- und Ausstiegs-Legs** (jeweils mit eigenem Preis, eigener Menge, eigenen Gebühren, eigenem Signal), plus **SL/TP-Brackets**. Löst ein Bracket aus, hake es ab, und es wird zu einem Ausstiegs-Leg gefaltet.

Das Formular zeigt beim Tippen eine Vorschau von durchschnittlichem Einstieg, Netto-PnL und offener Menge. Du kannst bis zu zwei Bilder anhängen (Chart-Screenshots), Hebel und Kontraktmultiplikator für Derivate wählen und dein eigenes Feedback zum Trade schreiben.

**PnL wird beim Lesen berechnet** und verarbeitet teilweise offene Positionen. Die Einstandsbasis ist umschaltbar zwischen **gewichtetem Durchschnittskurs** (Standard) und **FIFO** (nützlich für den Steuerexport), und die Wahl wird tatsächlich angewendet, sowohl in den Statistiken als auch in der Live-PnL-Vorschau des Trade-Formulars.

## Gebühren

Speichere unter **Gebühren & Währung** **Gebührenmodelle**: fest oder prozentual, berechnet pro Lot, Einheit, Kontrakt oder Trade (z. B. *IBKR-Aktien: 0,05 % pro Trade*). Die Wahl eines Modells bei einem Trade berechnet die Gebühr automatisch; eine manuell eingegebene Gebühr gewinnt immer.

## Mehrwährung & FX

Trades behalten die Währung, in der du sie eingegeben hast. Die Auswahl **Anzeige** im Seitenkopf von Aufschlüsselung, Analysen, PnL-Kalender, Trades und Was-wäre-wenn-Labor wählt die Währung, in die sie umgerechnet werden, über einen täglichen FX-Feed, der Kurse jeden Geschäftstag automatisch nachträgt und Kurse über Wochenenden und Feiertage fortschreibt.

Ein Kurs wird in dieser Reihenfolge gesucht:

1. Die Online-Quelle (EZB-Referenzkurse), die die wichtigsten Währungen abdeckt.
2. Eine in [Historical Data](/de/modules/market-data#histdata) gespeicherte Tagesreihe, für Währungen, die die Online-Quelle nicht notiert (USDT, USDC). Standardmäßig werden die üblichen Paare versucht. Greifen sie nicht, nimmt **Markt benennen** unter **Offene Aufgaben** eine bereits heruntergeladene Reihe oder einen Konnektor und das Paar so, wie dieser Handelsplatz es schreibt (Binance: `USDTUSD`), und dieser Markt wird dann an jedem Datum verwendet. **Reihe herunterladen** holt eine, wenn noch nichts gespeichert ist.
3. Ein Kurs, den du von Hand eintippst.

Kann keine Quelle ein Datum bepreisen, werden diese Trades **aus den umgerechneten Summen ausgeschlossen** und erscheinen unter **Offene Aufgaben**, wo du die fehlenden USD-basierten Kurse von Hand einträgst (1 USD = … dieser Währung), und die Trades zählen wieder. Ein von Hand eingetippter Kurs gewinnt immer.

**Offene Aufgaben** listet außerdem die Instrumente, die eine [Broker-Synchronisierung](#pull-from-a-broker) zurückgehalten hat, damit du sie beantwortest.

## Aufschlüsselung (deine Statistiken)

Pro Kategorie oder über alle, filterbar nach Datumsbereich, Ticker, Seite, Anlageklasse, Strategie, Signal und Tag. Die Filterleiste wird mit Analysen geteilt, ein auf einem Bildschirm gesetzter Bereich gilt also auch auf dem anderen, und die Ticker-Liste bietet die Symbole, die das Journal tatsächlich hält:

- **Details**: Bei einer Kategorie enthält ein einklappbares Banner oben eine bearbeitbare Beschreibung (ihre These, Regeln, Umfang).
- **Equity-Kurve** in der Anzeigewährung oder in Prozent des Kapitals, mit den Zeitraum-Schaltflächen **7T**, **1M**, **6M** und **1J** (ein erneuter Klick auf die aktive zeigt die gesamte Historie).
- Investiertes Kapital · Aktuelles Eigenkapital · Realisierter PnL · Rendite · Trefferquote · Trades (geschlossen/offen) · Expectancy · Profit Factor · Ø Gewinn / Ø Verlust · Bester / Schlechtester Trade · Max. Drawdown · Sharpe und Sortino · Gebühren gesamt · Max. eingesetzte Margin · Rendite auf Margin.
- **Sharpe und Sortino** werden auf Tagesrenditen gegen das in jeden Tag mitgenommene Eigenkapital berechnet und annualisiert, dieselbe Definition wie in Analysen, sodass beide Ansichten übereinstimmen.

## Analysen (das Buch lesen)

Das ganze Buch in zehn Tabs, über dieselben Filter wie die Aufschlüsselung:

- **Überblick**: Expectancy in **R** und Gesamt-R (über die Trades, die einen geplanten Stop tragen), Risiko pro Trade, Sharpe mit Sortino daneben, max. Drawdown mit den Tagen unter dem Hoch, gewonnene und verlorene Handelstage, durchschnittlicher Tag, aktuelle und beste Serie, dann die **R-Verteilung** und die Kosten deiner getaggten Fehler.
- **Verteilungen**: Netto-PnL nach Haltedauer, Einstiegsstunde, Einstiegswochentag und Positionsgröße sowie die Trade-Anzahl pro Gewinn-Bucket. Welche Stunde deines Tages sich tatsächlich auszahlt.
- **Verhalten**: was du rund um den Edge tust und was es kostet. Gewinnkonzentration, Größe nach einer Verlustserie, Tempo nach einem Verlust, wie der Tag zerfällt, und der Trade nach einem Gewinn gegenüber dem Trade nach einem Verlust. Siehe [Verhaltensanalyse](#behavior).
- **Marktdaten**: die Candles hinter deinen Trades. MAE und MFE, Exit-Effizienz, was auf dem Tisch blieb, Stop-Abstand in ATR und Ergebnisse, aufgeteilt nach Volatilitätsregime und nach Trend beim Einstieg. Siehe [Marktdaten-Anreicherung](#market-data).
- **Offenes Risiko**: der einzige Tab über die Gegenwart. Was noch auf dem Spiel steht, wo sich dieses Risiko konzentriert und ob fünf offene Linien fünf Wetten oder eine sind. Siehe [Offenes Risiko](#open-risk).
- **Streudiagramm**: zwei beliebige Trade-Werte gegeneinander aufgetragen (Datum, Trade-Nummer, Netto, kumulierter PnL, Rendite auf Notional, R, Haltedauer, Größe...), eingefärbt nach Ergebnis, Seite, Strategie, Ticker oder Anlageklasse, mit Trendlinie und Zoom.
- **3D-Streudiagramm**: drei Trade-Werte auf drei Achsen, dazu die Marktsitzung des Einstiegs, in einem Würfel, den du von Hand drehst und zoomst. Es zeigt, ob zwei Variablen nur zusammen zählen.
- **Serien 3D**: jede Gewinn- und Verlustserie Trade für Trade gezeichnet, in Geld oder in R, ab null, sodass sich ihre Formen direkt vergleichen lassen.
- **Aufschlüsselung**: Performance gruppiert nach Strategie, Symbol, Tag, Anlageklasse oder Seite, mit Trades, Trefferquote, Netto, Expectancy, Ø R und Profit Factor pro Zeile.
- **Vergleichen**: dieser Tag, diese Woche, dieser Monat, dieses Quartal, dieses Jahr oder ein eigener Bereich gegenüber dem direkt davor, Zeile für Zeile (Netto, Trades, Trefferquote, Expectancy, Ø R, Profit Factor, max. Drawdown, Gebühren, Handelstage), über einem Streifen der letzten zwölf Perioden.

Geschlossene Trades ohne FX-Kurs für ihr Datum werden ausdrücklich mitgezählt statt stillschweigend verworfen.

## Verhaltensanalyse {#behavior}

Performance-Statistiken sagen, was das Buch zurückbrachte. **Verhalten** sagt, wie du dorthin kamst und welche deiner Gewohnheiten dafür bezahlt haben. Dieselben geschlossenen Trades wie im Rest der Analysen, dieselbe Filterleiste, keine zusätzliche Einrichtung und keine Marktdaten: Es liest die Trades, die du bereits erfasst hast.

Fünf Karten, jede beantwortet eine Frage.

### Woher der Gewinn kommt

Der Anteil am Bruttogewinn, den deine fünf besten Trades machen, wie viele Gewinner es braucht, um die Hälfte zu erreichen, und wie das Konto ohne diese fünf aussieht. Daneben ein Konzentrationsindex: 0 heißt, jeder Gewinner zahlt etwa gleich viel, 1 heißt, ein einzelner Trade zahlt das Jahr. Mittlerer Gewinn gegenüber Median-Gewinn zeigt dieselbe Schiefe aus einem anderen Blickwinkel, und eine kumulative Kurve zeichnet sie.

Die Zahl, auf die es ankommt, ist das Netto ohne die Top Fünf. Ist sie negativ, ruht der Edge auf Ausreißern, die du nicht planen kannst.

### Größe nach einer Verlustserie

Median des Einstiegs-Notionals, gruppiert danach, was vor dem Trade kam: nach einem Gewinn, nach einem Verlust, nach zweien, nach drei oder mehr. Jede Zeile trägt ihre Trade-Anzahl, Trefferquote, Expectancy, Ø R und Netto, sodass die Eskalation beziffert statt nur bemerkt wird.

Nach zwei Verlusten die Größe zu erhöhen ist die teuerste Gewohnheit, die ein Journal erwischt. Eine flache Zeile hier ist die Disziplin, die die meisten Bücher zuerst verlieren.

### Tempo nach einem Verlust

Der Median-Abstand von einem Ausstieg zum nächsten Einstieg, verglichen nach einem Gewinn und nach einem Verlust. Ein Trade, der direkt nach einem Verlust in weit weniger als **deinem eigenen** üblichen Abstand eröffnet wird, zählt als Revenge-Trade, denn ein Scalper und ein Swing-Trader teilen keine Uhr. Diese Trades bekommen eine eigene Zeile: wie viele, was sie brachten und was sie im Schnitt gegenüber allem anderen bringen.

Auch Tage werden verglichen, ein Tag mit einem Verlust gegenüber einem sauberen, nach Trade-Anzahl.

### Wie der Tag verläuft

Durchschnittliches Ergebnis nach dem Rang des Trades innerhalb seines lokalen Tages: erster, zweiter, dritter, vierter und danach, mit Ø R pro Rang und dem, was jeder weitere Trade des Tages wert ist. Viele Bücher verdienen ihr Geld vor dem Mittagessen und geben es danach zurück. Hier zeigt sich das.

### Nach einem Gewinn, nach einem Verlust

Der Trade, der einem Ergebnis **folgt**, nie das Ergebnis selbst. Trades, Trefferquote, Expectancy, Ø R, Median-Größe, durchschnittliches Risiko, Median-Haltedauer und Median-Abstand nebeneinander, mit den drei Abständen, auf die es ankommt (Expectancy, Größe, Haltedauer), darunter hervorgehoben.

::: tip Aussagen haben eine Untergrenze
Der Satz oben auf einer Karte wird nur oberhalb einer **Stichproben-Untergrenze** geschrieben (zwanzig geschlossene Trades im Bereich, acht auf jeder Seite eines Vergleichs) **und** einer Effektschwelle. Darunter werden die Karten weiterhin gezeichnet und als erster Eindruck beschriftet. Drei Trades können keine Gewohnheit zeigen.
:::

## Marktdaten-Anreicherung {#market-data}

Das Trade-Log kennt deinen Einstieg, deinen Ausstieg und deinen Stop. Es weiß nicht, wohin der Kurs lief, während du drin warst, und dort liegen die meisten nützlichen Antworten: ob deine Stops innerhalb des Rauschens liegen, wie viel jeder Bewegung du tatsächlich behalten hast und unter welchen Marktbedingungen die Strategie funktioniert.

Der Tab **Marktdaten** lädt die Candles hinter deinen eigenen Trades und vermisst sie.

### Einmal einrichten

Öffne **Quellen** im Tab:

- **Candle-Größe**: *Automatisch* wählt den gröbsten Timeframe, der noch etwa zwanzig Candles innerhalb einer typischen Position lässt, abgelesen an deiner eigenen Median-Haltedauer. Lege eine fest, wenn du lieber selbst entscheidest.
- **Abruf**: *Aus* vermisst nur, was schon gespeichert ist, *Auf Anforderung* lädt beim Klick, *Automatisch* reiht das fehlende Fenster eines neuen Trades von selbst ein und benachrichtigt dich, wenn es ankommt.
- **Quelle pro Anlageklasse**: Aktien, ETFs, Krypto, Forex und Futures wählen jeweils einen [Daten-Konnektor](/de/config/connectors), der dem Journal gewährt ist, oder *Automatisch*, was den ersten gewährten Konnektor nimmt, der diese Klasse bedient.

Nichts wird hinter deinem Rücken heruntergeladen, und die Downloads sind gewöhnliche [Historical Data](/de/modules/market-data#histdata)-Jobs: dieselbe Warteschlange, dieselbe Kontingentzählung, dieselbe Jobliste.

### Entdecken, herunterladen, vermessen

Drei Schaltflächen, in dieser Reihenfolge.

- **Entdecken** liest, was die gefilterten Trades brauchen, gegen das, was du bereits gespeichert hast, und schreibt nichts. Pro Instrument erhältst du die Trades im Bereich, die Bars im Speicher, die fehlenden Fenster und einen Status: *bereit*, *teilweise*, *fehlt*, *keine Quelle*, *nicht unterstützt*, *Kontrakt nötig*, *ohne Daten*.
- **Fehlendes herunterladen** reiht diese Fenster ein und verfolgt den Batch. Es werden nur die Lücken angefragt, und eine Lücke wird an den Bars bemessen statt aus einem Abstand geraten: Eine tägliche Aktienserie fehlt an jedem Wochenende, eine 24/7-Kryptoserie nie, daher funktioniert keine Lückenbreite für beide.
- **Vermessen** lässt jeden Trade gegen seine Bars laufen und speichert das Ergebnis.

Das Vermessen ist **inkrementell**. Eine gespeicherte Messung wird wiederholt, wenn der Trade bearbeitet wurde, neue Candles eintrafen oder sich die Granularität änderte. *Alle neu vermessen* erzwingt den ganzen Bereich, wenn du die Candle-Größe änderst und jeden Trade auf dieselbe Basis stellen willst.

### Einen Futures-Kontrakt benennen

Eine Aktie heißt überall gleich. Ein Futures-Kontrakt nicht, und dein Journal-Ticker benennt meist die Wurzel, die du handelst, statt den Kontrakt, den dein Datenanbieter bedient.

Das Journal fragt daher einmal, statt zu raten. Ein Instrument, das das braucht, zeigt *Kontrakt nötig*, und **Kontrakt benennen** nimmt das Symbol so, wie deine Quelle es schreibt: `MNQU6` (was TWS zeigt und was du kopierst) oder die Wurzel mit ihrem Kontraktmonat, `MNQ.202609`, oder `MNQ.202609@CME`, wenn die Wurzel an mehreren Börsen notiert ist. Alles Nachgelagerte nutzt dieses Symbol.

Optionen werden als **nicht unterstützt** gemeldet, statt ihrem Basiswert zugeordnet zu werden. Einen Options-Trade gegen die Candles der Aktie zu vermessen würde Zahlen liefern, die richtig aussehen und nichts bedeuten.

### Was du bekommst

- **Exkursionen**: durchschnittliches MAE und MFE, in Geld und in Einheiten des geplanten Risikos, mit der Mediandauer vom Einstieg bis zu jedem.
- **Exit-Effizienz**: der Anteil der besten Bewegung, den du tatsächlich behalten hast, und was über alle vermessenen Trades hinweg auf dem Tisch blieb.
- **Sind die Stops zu eng**: Median-Stop-Abstand in ATR beim Einstieg, wie viele Stops unter einer ATR liegen und wie viele *Gewinner* zuerst über 80 % ihres Risikos hinausgingen. Ein Stop innerhalb des Rauschens ist ein Stop, den der Markt auf dem Weg zu deinem Ziel mitnimmt.
- **Sind die Ziele zu nah**: Gewinner, die unter der Hälfte der angebotenen Bewegung behielten, und was am besten Punkt zu sehen war gegenüber dem, was nach Hause kam.
- **Wie tief, bevor es funktioniert**: der schlechteste Punkt jedes Trades in R-Buckets, von 0 bis 0,25R bis mehr als 1,5R. Das sagt dir, wohin ein Stop gehört.
- **Nach Volatilitätsregime** und **nach Trend beim Einstieg**: dieselben Statistiken aufgeteilt in ruhig / normal / volatil und steigend / flach / fallend.

Regime sind **Terzile deines eigenen Buchs**, keine absoluten Schwellen. Eine absolute Schwelle würde jeden Krypto-Trade volatil nennen und nichts darüber sagen, wann deine Strategie funktioniert. Unter zwölf vermessenen Trades wird gar kein Regime beschriftet.

Auch der Tab Streudiagramm liest diese: MAE gegen R, Effizienz gegen Haltedauer, die Wolke nach Regime eingefärbt.

## Offenes Risiko {#open-risk}

Jeder andere Tab vermisst die Vergangenheit. **Offenes Risiko** vermisst, was jetzt noch auf dem Spiel steht.

Eine Position ist offen, wenn Menge übrig ist, und sie wird mit diesem **Rest** gezählt: Ein Trade, der zu drei Vierteln abgestockt wurde, trägt ein Viertel des Risikos, nicht das Risiko, mit dem er eröffnet wurde.

### Die Schlagzeile

- **Brutto- und Netto-Exposure**, in Geld und als Anteil des Kontos, Longs und Shorts addiert und dann saldiert.
- **Auf dem Spiel**: was du verlierst, wenn jeder geplante Stop getroffen wird. Positionen ohne erfassten Stop werden separat gezählt und benannt, denn was sie riskieren, ist unbekannt, nicht null.
- **Offenes Ergebnis**, zum zuletzt gespeicherten Schlusskurs bewertet, mit der Angabe, wie viele Positionen tatsächlich bewertet werden konnten.
- **Effektive Wetten**: wie vielen unabhängigen Positionen deine Linien entsprechen, nach Größe und erneut bei ihren gemessenen Korrelationen.

Die Positionstabelle listet sie nach Größe, die größte zuerst, mit Seite, offener Größe, Einstieg, Stop, letztem Preis, Wert, dem, was auf dem Spiel steht, ihrem Anteil am Ganzen, offenem Ergebnis und Haltetagen.

### Wo das Risiko sitzt

Konzentration nach Instrument, Anlageklasse, Seite oder Strategie, berechnet auf das **Risiko**, wenn Stops erfasst sind, und mit Rückfall auf die Größe, wenn nicht (das Panel sagt, welches). Ein Instrument, das die Hälfte dessen trägt, was auf dem Spiel steht, ist eine Tatsache über dein Buch, die keine Equity-Kurve zeigt.

### Sind das getrennte Wetten

Fünf Linien, die sich gemeinsam bewegen, sind eine Position in fünffacher Größe. Um das zu beantworten, misst der Tab die Korrelation auf **bereits gespeicherten Tageskerzen**, welche Granularität die Anreicherung auch nutzte, und holt nie etwas.

Drei Zahlen, und nur die dritte beschreibt dein Buch:

- **Addiert**: jeder Stop gleichzeitig getroffen, summiert.
- **Bei Unabhängigkeit**: wie das Risiko wäre, wenn nichts sich gemeinsam bewegte.
- **Bei diesen Korrelationen**: was das Buch tatsächlich riskiert.

Ihr Verhältnis ist der **Stacking**-Faktor: 1,0 heißt wirklich getrennte Wetten, höher heißt dieselbe Wette mehrfach. Instrumente ohne gespeicherte Candles werden benannt und aus der Matrix ausgelassen statt angenommen.

Warnungen lesen sich als Sätze: ein Paar, das sich bei 0,9 bewegt, ein einzelnes Instrument, das zu viel trägt, Positionen ohne Stop, ein Buch, das dünner ist, als es aussieht. Ist nichts falsch, wird auch das gesagt.

## Was-wäre-wenn-Labor {#lab}

Das **Was-wäre-wenn-Labor** spielt die geschlossenen Trades der geöffneten Kategorie mit einer einzigen Änderung nach und stellt das Ergebnis neben das, was du tatsächlich getan hast: dieselben Zahlen vorher und nachher (Trades, Netto, Gebühren, Trefferquote, pro Trade, Profit Factor, max. Drawdown, schlechtester Tag), beide kumulierten Kurven auf einer Achse und jeden Trade mit seiner Änderung. Wo die Änderung einen Trade auslässt, bleibt die Kurve flach.

Ein Tab pro Änderung:

- **Ausstiege**: andere Stops, Ziele und Exit-Signale, siehe [unten](#exit-lab).
- **Größe**: jeder Trade mit dem 0,5- bis 3-Fachen seiner Größe, die Gebühren entsprechend skaliert. Eine optionale **tägliche Verlustgrenze** sperrt neue Einstiege für den Rest des Tages, sobald der realisierte Verlust sie erreicht.
- **Filter**: die in einem Einstiegszeitfenster (Stunden, Wochentage) eröffneten Trades behalten oder verwerfen, und nur die behalten, bei denen eine Indikatorbedingung auf der letzten beim Einstieg geschlossenen Candle galt. Nur eingebaute Indikatoren, mit derselben Mathematik wie Backtests und Chart-Alarme.
- **Umkehren**: dieselben Einstiege und Ausstiege mit umgedrehter Seite, bei jedem Trade oder nur bei denen, die einen Filter erfüllen. Die Gebühren bleiben wie bezahlt.
- **Gebühren**: die bezahlten Gebühren gegenüber einem deiner eigenen [Gebührenmodelle](#gebuhren), berechnet so, wie das Trade-Formular sie berechnet. Mit einer Kontraktfamilie (Micro- und Standard-Futures) wechseln die Trades auf den anderen Kontrakt, und Kontrakte, die keinen ganzen ergeben, behalten ihre eigenen Gebühren.

Nichts wird geraten. Ein Trade ohne FX-Kurs, ohne Einstiegszeit für ein Fenster oder ohne gespeicherte Candle für eine Bedingung wird als **nicht vermessen** aufgeführt, mit dem Grund, und auf beiden Seiten weggelassen. Das Urteil in einem Satz erscheint ab 20 geschlossenen Trades. Darunter lies die Zahlen als Skizze.

### Ausstiege {#exit-lab}

Der Tab **Ausstiege**, auch über **SL/TP-Signale** im Tab Trades geöffnet, spielt deine geschlossenen Trades unter einfachen Stop-, Ziel- und Exit-Regeln nach (Stops in Prozent, in ATR und nachlaufend, Ziele in R, Exits per RSI, Bollinger, gleitendem Durchschnitt und MACD, Zeit-Exits...). Einstieg, Größe und Gebühren bleiben deine, nur der Exit bewegt sich, also ist jede Zahl der Preis des Exits. Er liest die Candles, die der Tab [Marktdaten](#market-data) speichert, höchstens 500 Trades pro Analyse.

- **Was getestet wird**: jede vorgefertigte Regel des Katalogs in einem Lauf (28 davon), plus jedes Signal, das mit **Im Labor öffnen** hereingebracht wurde.
- **Wie ein Signal angewendet wird**: *Zusätzlich zu meinem Ausstieg* lässt ein Signal einen Trade nur vor deinem echten Exit schließen. *Ersetzt meinen Ausstieg* macht das Signal zum Exit, und **Bars nach dem Ausstieg** legt fest, wie viele Candles nach deinem echten Exit es den Trade noch schließen darf (standardmäßig 20, höchstens 500).
- Die Signale werden nach Netto-PnL, niedrigstem Drawdown, Trefferquote, durchschnittlichem R oder Profit Factor gegen **Meine echten Ausstiege** gereiht, mit dem besten Stop, dem besten Ziel und dem besten Stop + Ziel hervorgehoben, kumulierten Kurven und den Trades hinter jeder Zeile.

### Strategie-Bericht

**Bericht** bei einer Strategie unter **Strategien & Kapital** fasst die geschlossenen Trades zusammen, die sie tragen: Trades, Netto, pro Trade, Trefferquote, durchschnittliches R, Profit Factor und max. Drawdown, dann die R-Verteilung und das Ergebnis nach Einstiegsstunde, Wochentag, Volatilitätsregime und Instrument.

Darunter laufen vier Standard-Was-wäre-wenn-Szenarien auf denselben Trades: ein Stop bei der medianen adversen Bewegung der Gewinner, ein Ziel beim 75. Perzentil der günstigen Bewegung, die schlechteste Einstiegsstunde gestrichen und die 1,5-fache Größe, unter der täglichen Verlustgrenze, die du auf dem Bericht eingibst (standardmäßig keine). Stop und Ziel brauchen die von [Marktdaten](#market-data) vermessenen Exkursionen. **Im Labor öffnen** öffnet jedes davon im Labor, mit ausgefüllten Einstellungen.

## Insights {#insights}

Die Schaltfläche **Insights** im Kopf des Journals, bei einem gespeicherten Backtest-Lauf und auf dem Dashboard eines [Trading-Bots](/de/modules/robots) öffnet eine Seite, auf der ein fester Satz von Researchern die geschlossenen Trades liest. Jeder Befund ist ein Satz, mit seinen Belegen: wie viele Trades und was er am Ergebnis, am Drawdown oder am Risiko pro Trade ändert.

Wonach sie suchen:

- **Stunden und Wochentage**: die schlechteste Einstiegsstunde oder der schlechteste Einstiegstag, und der beste, mit 1,5-facher Größe.
- **Haltedauer**: Verlierer länger gehalten als Gewinner, oder Gewinner, denen Raum zum Laufen gelassen wird.
- **Stop-Platzierung**: Stops enger, als die adversen Bewegungen der Gewinner es verlangen, oder weiter als nötig.
- **Konzentration**: ein Ticker oder eine Anlageklasse, die das Buch nach unten zieht oder die meisten Trades trägt. Der Sektor wird nur genutzt, wenn für das Unternehmen ein SEC-Branchencode gespeichert ist.
- **Gebühren**: was eines deiner Gebührenmodelle berechnet hätte, auch auf dem anderen Kontrakt einer Familie. Ohne Modell sagt die Seite das und verlinkt auf Gebühren.
- **Größe nach Verlusten** (nur Backtest-Läufe und Bot, die [Verhaltensanalyse](#behavior) des Journals deckt das bereits ab): die Positionsgröße nach zwei Verlusten in Folge gegenüber der Größe nach einem Gewinn.
- **Long gegen Short**: eine Seite verliert, während die andere verdient.

Wie weit du dich darauf verlassen kannst:

- Unter 20 geschlossenen Trades wird nichts ausgesagt. Darunter schlägt die Seite den nächsten Schritt vor: Trades importieren oder erfassen, einen längeren Backtest ausführen oder den Bot länger handeln lassen. Ein Befund muss außerdem mindestens 5 % des Umsatzes bewegen (die Summe der absoluten Nettoergebnisse), mit genug Trades auf jeder Seite des Vergleichs. *Nichts sticht hervor* ist eine echte Antwort.
- Im Journal werden die Aussagen zu Verhalten und offenem Risiko unverändert neben den Befunden aufgeführt.
- **Im Labor öffnen** öffnet das Was-wäre-wenn-Labor des Journals mit ausgefüllter Änderung, sodass du die Aussage Trade für Trade prüfen kannst. Bei einem Lauf oder einem Bot bietet ein Befund zu Stunde oder Wochentag **Als Filter hinzufügen**: Die Strategie öffnet sich mit diesem Handelsfenster als ungespeicherter Entwurf. Das Fenster wird dabei immer nur enger.
- Bei einem Lauf oder einem Bot sind Beträge in dessen Kontowährung, und Gebührenmodelle mit festem Betrag werden nicht eingepreist.
- Ein Backtest-Lauf muss zuerst gespeichert werden. Die Quelle Journal braucht das Modul Trading Journal, Läufe brauchen Backtest und Bots brauchen Trading-Bots.

## Disziplin-Tags

Ein **Tag** ist eine Regel, die du gebrochen oder eingehalten hast: *Stop verschoben*, *kein Setup*, *nach Verlust Größe erhöht*. Hake sie bei den Trades ab, auf die sie zutreffen, und Analysen beziffert sie: wie viele geschlossene Trades eine Regel brachen, was diese Trades im Schnitt gegenüber den sauberen bringen, und die Lücke dazwischen. Das sind die **Kosten von Fehlern**, in Geld.

## PnL-Kalender

Ein Monatsraster des täglich realisierten PnL, grün für Plustage und rot für Minustage, auf den größten Tag des Monats skaliert, mit Wochensummen am Rand. Der Einheitenschalter zeigt ihn in der Anzeigewährung oder in Prozent des Kapitals, jeder Tag gemessen am Kapital, das in ihn mitgenommen wurde (investiertes Kapital plus das zuvor Realisierte). Klicke einen Tag an, um seinen [Tagesbericht](#day-report) zu öffnen.

Er liest auch deine **Trading Routines**. Hänge die Routinen, denen eine Kategorie folgt, über einen Zeitraum an, und jeder gehandelte Tag trägt einen Punkt: grün, wenn jede an diesem Tag fällige Routine abgehakt war, rot, wenn keine, bernstein dazwischen. Ein Tag, an dem du nicht gehandelt hast, bleibt grau, was die Routinen auch sagen, und ein Tag ohne fällige Routine bekommt gar keinen Punkt. Beim Darüberfahren wird jede Routine mit ihrer eigenen Markierung genannt.

Die Routinen selbst leben in [Trading Routines](/de/modules/productivity#routines) und werden dort abgehakt: Das Journal vermerkt nur, welche ein Buch führt, sodass dieselbe Gewohnheit nie zweimal geschrieben oder abgehakt wird.

Er liest deine [Mindset](/de/modules/productivity#mindset)-Check-ins auf dieselbe Weise. **Mindset** wählt im Kalender einer Kategorie die Check-ins, denen dieses Buch folgt. Jeder Tag trägt dann ihren Score, und ein gehandelter Tag, an dem keiner ausgefüllt wurde, wird als **verpasst** markiert. Ein ausgelassener Check-in an einem Tag ohne Handel ist kein Versäumnis. Klicke auf das Badge, um den Check-in dieses Tages in Mindset zu öffnen.

Routinen und Mindset sind außerdem mit einem Klick in der Seitenleiste des Journals erreichbar.

### Tagesbericht {#day-report}

Ein Tag des Kalenders öffnet seinen Bericht, mit Pfeilen zum vorherigen und nächsten Handelstag:

- **Kacheln**: Netto-P&L mit Brutto und Gebühren, Trefferquote, Profitfaktor und Erwartungswert, Payoff, Intraday-Drawdown, R gesamt und durchschnittliche Haltedauer.
- **Intraday-P&L**: das realisierte Ergebnis des Tages, Trade für Trade, mit jedem Trade beim Darüberfahren.
- **Tagesdetails**: bester und schlechtester Trade, Ausführungen, noch offene Positionen, erste bis letzte Ausführung und als Fehler markierte Trades, dann der Tag je Instrument.
- **Positionen** des Tages und die an ihm **ausgeführten Orders**.
- **Routine** und **Mindset**: die an diesem Tag fälligen Routinen und die ausgefüllten Check-ins, jeweils mit eigenem Blatt.

**In der Trade-Liste bearbeiten** öffnet die Trades des Tages zum Bearbeiten. Die Check-in-Seite in Mindset verlinkt mit **Journal-Tag** hierher zurück.

## Ein Trade-Buch importieren {#import-a-trade-book}

Die Ansicht **Import** nimmt ein Trade-Buch, das du anderswo führst (ein Broker-Export, ein anderes Journal, eine Tabelle), und macht daraus Journal-Trades. Kein Parser pro Broker: CSV, TSV, JSON und Parquet laufen alle durch dieselbe Erkennung.

**So funktioniert es.** Ziehe die Datei hinein, der Server schlägt eine Zuordnung vor, du prüfst sie an einer Vorschau echter Trades, dann importierst du.

- **Erkennung** liest die Header (en, fr, es, de, it, pt, plus gängige Broker-Begriffe) *und* die Werte selbst. Trennzeichen, Dezimaltrennzeichen und Tag-zuerst- gegenüber Monat-zuerst-Daten werden pro Spalte entschieden. Eine Spalte, die sie nicht sicher identifizieren kann, bleibt **nicht zugeordnet**, statt geraten zu werden, und du ordnest sie selbst zu.
- **Zeitzone.** **Zeiten sind in** wählt die Zone, in der die Zeiten der Datei geschrieben wurden: eine benannte Zone, die der Sommerzeit folgt, oder ein fester Versatz. Standard ist die Zone deines Browsers.
- **Vorschau vor dem Schreiben.** Der Analyseschritt schreibt nichts: Er liefert die gebauten Trades, die Summen und die Fehler pro Zeile und läuft bei jeder Änderung der Zuordnung neu, was du siehst, ist also genau das, was gespeichert wird. Der eigene P&L der Datei wird gegen den berechneten gegengeprüft.
- **Zeilenform.** Eine Zeile ist entweder ein **Round Trip** (eine Zeile = ein Trade) oder eine **Ausführung** (eine Zeile = ein Fill). Ausführungen werden pro Instrument zu Positionen mit Einstiegs- und Ausstiegs-Legs gruppiert; was am Ende noch offen ist, wird als offener Trade importiert.
- **Gestapelte Auszüge.** Ein Export, der mehrere Tabellen in einer Datei bündelt (im IBKR-Stil), wird Abschnitt für Abschnitt gelesen, mit einer Auswahl, um die Tabelle zu wechseln oder die Datei flach zu lesen.
- **Punktwert.** Kein Export trägt, was ein Punkt eines Instruments wert ist. Hake **Punktwert pro Ticker festlegen** an, um ihn für jeden in der Datei gefundenen Ticker einzugeben (Futures, CFDs); ohne Haken zählt jeder Ticker 1, was für Aktien und Krypto stimmt. Er wird mit der Zuordnung gespeichert.
- **Zuordnungen.** Speichere eine Zuordnung, und die nächste Datei derselben Quelle wird an ihrem Header-Fingerabdruck erkannt und ordnet sich selbst zu. Von Hand korrigierte Spalten werden ebenfalls gemerkt.

::: tip Eine Zuordnung ist keine Vorlage
Eine Journal-**Vorlage** ist das Formular, mit dem du einen Trade von Hand erfasst. Eine Import-**Zuordnung** sagt, welche Spalte einer fremden Datei welches Trade-Feld ist. Sie werden getrennt aufgelistet und nie vermischt.
:::

### Vom Broker abrufen {#pull-from-a-broker}

Ist dem Journal ein [Broker-Konto](/de/config/brokers) gewährt, erledigt **Vom Broker abrufen** denselben Import ohne Datei: Wähle das Konto und einen Zeitraum, und die Fills kommen zu Positionen gefaltet zurück, in der Vorschau, bevor etwas geschrieben wird. Es gibt keine Zuordnung zu prüfen, eine API antwortet mit typisierten Feldern. Das erneute Ausführen mit einem weiteren Zeitraum aktualisiert die bereits importierten Positionen, statt sie zu verdoppeln. Hake **Automatisch synchronisieren** an, und der Sync läuft im gewählten Intervall von selbst weiter, ab dort, wo der letzte aufgehört hat; pausiert, neu getaktet oder gelöscht wird der Zeitplan unter **Automator → Jobs**. **Sync planen** im Tab Trades öffnet dasselbe Panel.

- **Nach** wählt die Kategorie, in der die Trades landen. Der Zeitraum wird eingetippt oder mit einem Klick gesetzt: **7 Tage**, **30 Tage**, **90 Tage**, **1 Jahr**.
- **Instrumente** ist bei den meisten Brokern optional (leer nimmt jedes Instrument des Zeitraums) und Pflicht bei einem Broker, der ein Instrument nach dem anderen beantwortet. **Aus dem Konto vorschlagen** füllt es aus.
- **Dieses Konto darf short gehen**: Ohne Haken wird ein Verkauf ohne offene Position als ein vor dem Zeitraum gekauftes Instrument gelesen, nicht als ein Short, den du nie eingegangen bist.
- **Zurückgehaltene Instrumente.** Ein Fill sagt Kauf oder Verkauf, nie Eröffnung oder Schließung. Ein Instrument, dessen Fills nicht aufgehen, wird zurückgehalten, während alles andere importiert wird, und im Dialog sowie unter **Offene Aufgaben** gelistet. Beantworte es mit dem Eröffnungskurs einer vor dem Zeitraum eröffneten Position, nimm die Fills, wie sie sind, oder lass das Instrument weg. Deine Antworten werden von jeder späteren Synchronisierung erneut angewendet.

**Einen Import rückgängig machen.** Jeder Import ist ein **Batch**, aufgelistet mit Datum, Datei und Trade-Anzahl. *Rückgängig machen* löscht genau die Trades, die er erstellt hat. Importe werden außerdem **pro Kategorie** dedupliziert, ein erneuter Import derselben Datei ändert also nichts (derselbe Auszug kann trotzdem zwei Kategorien speisen, da eine Kategorie ein Buch ist). *Behalten, lösen* eines Batches hebt diesen Schutz auf und macht seine Trades wieder gewöhnlich.

## Export & Berichte

**Exportieren**, im Seitenkopf von Aufschlüsselung, Analysen, PnL-Kalender (und seinem Tagesbericht) und Trades, exportiert deine Daten oder erzeugt einen Performance-Bericht. Der **Umfang** ist die auf der Seite geöffnete Kategorie oder **Alle Kategorien**. Der **Zeitraum** ist **Wöchentlich** oder **Monatlich**, mit Pfeilen, um zum vorherigen oder nächsten Zeitraum zu wechseln:

- **Trades-CSV**: die rohen Trades, für Tabellenkalkulationen oder Steuersoftware. Bietet außerdem **Gesamter Zeitraum**.
- **Performance-Bericht**: Trefferquote, Expectancy, Gebühren, Aufschlüsselung nach Strategie und Kategorie, Equity-Kurve, gerendert als **Markdown oder PDF**.

## Arbeitet zusammen mit

- **Tax Calculator**: lädt deinen realisierten Journal-PnL für ein Steuerjahr, aufgeteilt in Kapital-/Derivate-/Kryptogewinne.
- **Charts**: Die Auswahl **Journal** im Order-Ticket des Charts verknüpft das Instrument mit einer Journal-Kategorie oder legt mit **Neues Journal** eine an. Bei jedem Abgleich des Trading-Kontos werden die Broker-Fills auf diesem Instrument dort als Positionen gebucht, egal wer den Ausstieg gesendet hat. Eine Position bleibt in dem Journal, in dem sie eröffnet wurde, und was das Konto vor der Verknüpfung hielt, wird nie gebucht.
- **Historical Data**: Die Tabs Marktdaten und Offenes Risiko lesen Candles über die [Daten-Konnektoren](/de/config/connectors), die dem Journal gewährt sind, und laden, was fehlt, als gewöhnliche Jobs.
- **Trading Routines**: Der PnL-Kalender zeigt pro gehandeltem Tag, ob die Routinen, denen die Kategorie folgt, abgehakt waren.
- **Mindset**: Der PnL-Kalender zeigt den Check-in-Score jedes Tages und markiert die gehandelten Tage, an denen einer fehlt.
- **Dashboard**: Ein Quick-Trade-Widget erfasst einen Trade von der Startseite, und **Journal analytics**-Widgets, für eine Kategorie oder alle, zeigen Netto-PnL, Trefferquote, Equity-Kurve, offene Positionen, Verhaltenserkenntnisse und mehr.
- **RemindMe**: Füge von der Journal-Seite aus eine Erinnerung hinzu (z. B. wöchentliches Review).
