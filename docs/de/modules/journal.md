# Trading Journal

Erfasse jeden Trade, in jeder Währung, und erhalte ehrliche Performance-Statistiken: Equity-Kurve, Trefferquote, Expectancy, Profit Factor, Drawdown, Sharpe und mehr. Das Journal ist in zehn Tabs gegliedert: **Aufschlüsselung**, **Analysen**, **PnL-Kalender**, **Trades**, **Strategien & Kapital**, **Tags**, **Vorlagen**, **Gebühren & Währung**, **Import**, **Offene Aufgaben**.

## Kategorien

Trades leben in **Kategorien**: Ordner wie *Krypto-Scalping* oder *Langfristige Aktien*, jeder mit eigener Farbe, eigenem Kapital und eigenen Statistiken. Lege sie in der Kategorieleiste an; per Ziehen umsortieren. Das Löschen einer Kategorie löscht ihre Trades.

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

Trades behalten die Währung, in der du sie eingegeben hast. Die **Aufschlüsselungswährung** (Anzeige) wird über einen täglichen FX-Feed umgerechnet, der Kurse jeden Geschäftstag automatisch nachträgt und Kurse über Wochenenden und Feiertage fortschreibt.

Kann ein Kurs für ein Datum nicht geholt werden, werden diese Trades **aus den umgerechneten Summen ausgeschlossen** und erscheinen unter **Offene Aufgaben**, wo du die fehlenden USD-basierten Kurse von Hand einträgst (1 USD = … dieser Währung), und die Trades zählen wieder.

## Aufschlüsselung (deine Statistiken)

Pro Kategorie oder über alle, filterbar nach Datumsbereich, Ticker, Seite, Anlageklasse, Strategie, Signal und Tag. Die Filterleiste wird mit Analysen geteilt, ein auf einem Bildschirm gesetzter Bereich gilt also auch auf dem anderen, und die Ticker-Liste bietet die Symbole, die das Journal tatsächlich hält:

- **Equity-Kurve** in der Anzeigewährung.
- Realisierter PnL · Rendite · Trefferquote · Trades (geschlossen/offen) · Expectancy · Profit Factor · Ø Gewinn / Ø Verlust · Bester / Schlechtester Trade · Max. Drawdown · Sharpe und Sortino · Gebühren gesamt · Investiertes Kapital · Eingesetzte Margin · Rendite auf Margin.
- **Sharpe und Sortino** werden auf Tagesrenditen gegen das in jeden Tag mitgenommene Eigenkapital berechnet und annualisiert, dieselbe Definition wie in Analysen, sodass beide Ansichten übereinstimmen.

## Analysen (das Buch lesen)

Das ganze Buch in acht Tabs, über dieselben Filter wie die Aufschlüsselung:

- **Überblick**: Expectancy in **R** und Gesamt-R (über die Trades, die einen geplanten Stop tragen), Risiko pro Trade, Sharpe mit Sortino daneben, max. Drawdown mit den Tagen unter dem Hoch, gewonnene und verlorene Handelstage, durchschnittlicher Tag, aktuelle und beste Serie, dann die **R-Verteilung** und die Kosten deiner getaggten Fehler.
- **Verteilungen**: Netto-PnL nach Haltedauer, Einstiegsstunde, Einstiegswochentag und Positionsgröße sowie die Trade-Anzahl pro Gewinn-Bucket. Welche Stunde deines Tages sich tatsächlich auszahlt.
- **Verhalten**: was du rund um den Edge tust und was es kostet. Gewinnkonzentration, Größe nach einer Verlustserie, Tempo nach einem Verlust, wie der Tag zerfällt, und der Trade nach einem Gewinn gegenüber dem Trade nach einem Verlust. Siehe [Verhaltensanalyse](#behavior).
- **Marktdaten**: die Candles hinter deinen Trades. MAE und MFE, Exit-Effizienz, was auf dem Tisch blieb, Stop-Abstand in ATR und Ergebnisse, aufgeteilt nach Volatilitätsregime und nach Trend beim Einstieg. Siehe [Marktdaten-Anreicherung](#market-data).
- **Offenes Risiko**: der einzige Tab über die Gegenwart. Was noch auf dem Spiel steht, wo sich dieses Risiko konzentriert und ob fünf offene Linien fünf Wetten oder eine sind. Siehe [Offenes Risiko](#open-risk).
- **Streudiagramm**: zwei beliebige Trade-Werte gegeneinander aufgetragen (Datum, Trade-Nummer, Netto, kumulierter PnL, Rendite auf Notional, R, Haltedauer, Größe...), eingefärbt nach Ergebnis, Seite, Strategie, Ticker oder Anlageklasse, mit Trendlinie und Zoom.
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

- **Entdecken** liest, was die gefilterten Trades brauchen, gegen das, was du bereits gespeichert hast, und schreibt nichts. Pro Instrument erhältst du die Trades im Bereich, die Bars im Speicher, die fehlenden Fenster und einen Status: *bereit*, *teilweise*, *fehlt*, *keine Quelle*, *nicht unterstützt*, *Kontrakt nötig*.
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

## Disziplin-Tags

Ein **Tag** ist eine Regel, die du gebrochen oder eingehalten hast: *Stop verschoben*, *kein Setup*, *nach Verlust Größe erhöht*. Hake sie bei den Trades ab, auf die sie zutreffen, und Analysen beziffert sie: wie viele geschlossene Trades eine Regel brachen, was diese Trades im Schnitt gegenüber den sauberen bringen, und die Lücke dazwischen. Das sind die **Kosten von Fehlern**, in Geld.

## PnL-Kalender

Ein Monatsraster des täglich realisierten PnL, grün für Plustage und rot für Minustage, auf den größten Tag des Monats skaliert, mit Wochensummen am Rand. Klicke einen Tag an, um zu seinen Trades zu springen.

Er liest auch deine **Trading Routines**. Hänge die Routinen, denen eine Kategorie folgt, über einen Zeitraum an, und jeder gehandelte Tag trägt einen Punkt: grün, wenn jede an diesem Tag fällige Routine abgehakt war, rot, wenn keine, bernstein dazwischen. Ein Tag, an dem du nicht gehandelt hast, bleibt grau, was die Routinen auch sagen, und ein Tag ohne fällige Routine bekommt gar keinen Punkt. Beim Darüberfahren wird jede Routine mit ihrer eigenen Markierung genannt.

Die Routinen selbst leben in [Trading Routines](/de/modules/productivity#routines) und werden dort abgehakt: Das Journal vermerkt nur, welche ein Buch führt, sodass dieselbe Gewohnheit nie zweimal geschrieben oder abgehakt wird.

## Ein Trade-Buch importieren {#import-a-trade-book}

Die Ansicht **Import** nimmt ein Trade-Buch, das du anderswo führst (ein Broker-Export, ein anderes Journal, eine Tabelle), und macht daraus Journal-Trades. Kein Parser pro Broker: CSV, TSV, JSON und Parquet laufen alle durch dieselbe Erkennung.

**So funktioniert es.** Ziehe die Datei hinein, der Server schlägt eine Zuordnung vor, du prüfst sie an einer Vorschau echter Trades, dann importierst du.

- **Erkennung** liest die Header (en, fr, es, de, it, pt, plus gängige Broker-Begriffe) *und* die Werte selbst. Trennzeichen, Dezimaltrennzeichen und Tag-zuerst- gegenüber Monat-zuerst-Daten werden pro Spalte entschieden. Eine Spalte, die sie nicht sicher identifizieren kann, bleibt **nicht zugeordnet**, statt geraten zu werden, und du ordnest sie selbst zu.
- **Vorschau vor dem Schreiben.** Der Analyseschritt schreibt nichts: Er liefert die gebauten Trades, die Summen und die Fehler pro Zeile und läuft bei jeder Änderung der Zuordnung neu, was du siehst, ist also genau das, was gespeichert wird. Der eigene P&L der Datei wird gegen den berechneten gegengeprüft.
- **Zeilenform.** Eine Zeile ist entweder ein **Round Trip** (eine Zeile = ein Trade) oder eine **Ausführung** (eine Zeile = ein Fill). Ausführungen werden pro Instrument zu Positionen mit Einstiegs- und Ausstiegs-Legs gruppiert; was am Ende noch offen ist, wird als offener Trade importiert.
- **Gestapelte Auszüge.** Ein Export, der mehrere Tabellen in einer Datei bündelt (im IBKR-Stil), wird Abschnitt für Abschnitt gelesen, mit einer Auswahl, um die Tabelle zu wechseln oder die Datei flach zu lesen.
- **Punktwert.** Für jeden in der Datei gefundenen Ticker fragt der Import nach dem Kontrakt-Punktwert, da kein Export ihn trägt. Er wird mit der Zuordnung gespeichert.
- **Zuordnungen.** Speichere eine Zuordnung, und die nächste Datei derselben Quelle wird an ihrem Header-Fingerabdruck erkannt und ordnet sich selbst zu. Von Hand korrigierte Spalten werden ebenfalls gemerkt.

::: tip Eine Zuordnung ist keine Vorlage
Eine Journal-**Vorlage** ist das Formular, mit dem du einen Trade von Hand erfasst. Eine Import-**Zuordnung** sagt, welche Spalte einer fremden Datei welches Trade-Feld ist. Sie werden getrennt aufgelistet und nie vermischt.
:::

### Vom Broker abrufen

Ist dem Journal ein [Broker-Konto](/de/config/brokers) gewährt, erledigt **Vom Broker abrufen** denselben Import ohne Datei: Wähle das Konto und einen Zeitraum, und die Fills kommen zu Positionen gefaltet zurück, in der Vorschau, bevor etwas geschrieben wird. Es gibt keine Zuordnung zu prüfen, eine API antwortet mit typisierten Feldern. Das erneute Ausführen mit einem weiteren Zeitraum aktualisiert die bereits importierten Positionen, statt sie zu verdoppeln.

**Einen Import rückgängig machen.** Jeder Import ist ein **Batch**, aufgelistet mit Datum, Datei und Trade-Anzahl. *Rückgängig machen* löscht genau die Trades, die er erstellt hat. Importe werden außerdem **pro Kategorie** dedupliziert, ein erneuter Import derselben Datei ändert also nichts (derselbe Auszug kann trotzdem zwei Kategorien speisen, da eine Kategorie ein Buch ist). *Vergessen* eines Batches hebt diesen Schutz auf und macht seine Trades wieder gewöhnlich.

## Export & Berichte

Im Tab Trades kannst du deine Daten exportieren und einen Performance-Bericht erzeugen:

- **CSV-Export**: die rohen Trades, für Tabellenkalkulationen oder Steuersoftware.
- **Periodischer Bericht**: eine wöchentliche oder monatliche Performance-Zusammenfassung (Trefferquote, Expectancy, Gebühren, Aufschlüsselung nach Strategie und Kategorie, Equity-Kurve), gerendert als **Markdown oder PDF**.

## Arbeitet zusammen mit

- **Tax Calculator**: lädt deinen realisierten Journal-PnL für ein Steuerjahr, aufgeteilt in Kapital-/Derivate-/Kryptogewinne.
- **Historical Data**: Die Tabs Marktdaten und Offenes Risiko lesen Candles über die [Daten-Konnektoren](/de/config/connectors), die dem Journal gewährt sind, und laden, was fehlt, als gewöhnliche Jobs.
- **Trading Routines**: Der PnL-Kalender zeigt pro gehandeltem Tag, ob die Routinen, denen die Kategorie folgt, abgehakt waren.
- **Dashboard**: Ein Quick-Trade-Widget erfasst einen Trade von der Startseite.
- **RemindMe**: Füge Journal-bezogene Erinnerungen hinzu (z. B. wöchentliches Review).
