# Portfolios & Vermögen

Unabhängige Module, um zu verfolgen, was du beobachtest, was du besitzt, was es dich kostet und wie hoch die Steuerrechnung ausfallen könnte.

## Watchlists {#watchlists}

Benannte Listen von Symbolen, die du im Auge behalten willst: keine Positionen, kein Ledger, nur Kurse.

- **Symbole hinzufügen** per Suche (Krypto über CoinGecko, Aktien/ETFs über Yahoo), mit einer **kuratierten Vorlage** starten (Crypto Top 10, Magnificent 7, US-Index-ETFs, Halbleiter) oder **ein Portfolio aus dem Portfolio Tracker importieren**, wobei ein erneuter Import abgleicht statt zu duplizieren.
- Jede Zeile zeigt den Live-Kurs in USD, die **Änderungen über 24h / 3d / 7d / 30d**, eine **30-Tage-Sparkline**, die Börse und eine freie **Notiz** pro Symbol. Sortiere nach jeder Spalte, filtere nach Name.
- **Autoaktualisierung** pro Liste, von jeder Minute bis täglich (Standard 15 Min.). Die Seite schätzt die Anfragerate und **warnt, bevor ein Intervall die Drosselung kostenloser APIs riskiert**. Kurse werden serverseitig gecacht, das erneute Öffnen der Seite ist also sofort und erreicht die Anbieter nie.

### Eigene Kursquellen

Die öffentlichen Quellen (CoinGecko / Yahoo) funktionieren ohne Einrichtung. Hast du ein eigenes Marktdaten-Konto, stecke einen **[Daten-Konnektor](/de/config/connectors)** ein, dasselbe gemeinsame Anbieterkonto, das [Historical Data](/de/modules/market-data#histdata) und der Chart nutzen, einmal angelegt und den Watchlists gewährt. Jeder Konnektor trägt seine eigenen Zugangsdaten (eingetippt oder aus dem [Tresor](/de/config/settings#vault) gewählt) und sein eigenes Anfragelimit.

- Eine Liste kann **einen Konnektor als Standardquelle festlegen**, und jedes Symbol kann ihn überschreiben: *Liste folgen*, *auto* oder ein bestimmter Konnektor.
- **Anbieter-Ticker** pro Symbol (`BTCUSDT`, `AAPL.US`, …) werden automatisch abgeleitet und bleiben bearbeitbar, wenn ein Anbieter ein Symbol anders benennt.
- Ein fehlgeschlagener Kurs erscheint **in seiner eigenen Zeile**, sodass ein schlechtes Symbol den Rest der Liste nicht verbirgt.

::: warning Kenne die Limits deines Tarifs
Eine Liste mit eigener Quelle schaltet **Aktualisierungsintervalle von 5 s bis 30 s** frei. Die sind schnell genug, um einen API-Tarif rasch aufzubrauchen: Überzählige Aufrufe schlagen fehl und können deinen Schlüssel sperren lassen. Beobachte die Zähler unter **Einstellungen → API-Rate**.
:::

### Preisalarme

Jedes Symbol kann Alarme tragen, gesetzt über die Glocke in seiner Zeile. Jeder liest sich als ein Satz, den du von links nach rechts zusammenstellst, *benachrichtige mich, wenn BTC sich ±5 % von jetzt bewegt*:

- **Ein Level** (Preis über oder unter einem Wert) oder **eine Bewegung**, gemessen in **%** oder in **$**, nach oben, unten oder in beide Richtungen.
- Eine Bewegung wird **von jetzt an** gemessen oder über ein **gleitendes Fenster** (1h, 4h, 12h, 1d, 3d, 7d, 30d).
- **Einmalig oder wiederholend**, mit einer Wiederscharfschaltungs-Verzögerung (5m bis 1d), damit ein einzelner Ausschlag nicht bei jeder Aktualisierung feuert.
- **Ziele**: der In-App-Eingang plus jeder [Benachrichtigungskanal](/de/config/settings#notifications), den du pro Alarm wählst. Watchlists können nur Kanäle ansteuern, die ihnen gewährt wurden.

Alarme werden **serverseitig in der Aktualisierungsschleife** ausgewertet, sie feuern also bei geschlossener Seite und beendetem Browser.

### Listenbeschreibung

Eine Watchlist trägt unter ihrem Namen eine bearbeitbare Beschreibung dafür, wofür die Liste eigentlich gedacht ist.

## Portfolio Tracker {#portfolios}

Live-Wert deiner tatsächlichen Bestände, ein Portfolio pro Konto oder Thema.

- **Assets hinzufügen** per Suche (Krypto-Coins oder Aktien/ETFs) und **Kauf-/Verkaufsoperationen** erfassen (Datum, Menge, Preis, Gebühr, Notiz). Realisierter und unrealisierter G/V, Durchschnittskurs und Gewichte werden aus dem Ledger berechnet.
- **Handelswährung pro Asset**: Jedes Asset gibt die Währung an, in der seine Operationen erfasst werden, damit eine in EUR gekaufte Aktie nicht so geführt wird, als wäre sie USD. Formularbeschriftungen folgen dieser Währung. Spot-Kurse bleiben in USD: Einstandsbasis und realisierter G/V werden zum **Datum jeder einzelnen Operation** mit den FX-Kursen des [Trading Journal](/de/modules/journal) umgerechnet, ein Kauf von vor drei Jahren behält also seinen historischen Kurs. Ein Popover im Formular erklärt, woher jeder Preis stammt.
- **Autoaktualisierung**: Ein einmaliger Abgleichsschritt prüft jeden Bestand gegen seine Preisquelle; behebe alle, die *ungelöst* auftauchen (oder markiere sie als manuell), und aktiviere die **tägliche Autoaktualisierung**, danach werden die Preise täglich im Hintergrund aktualisiert.
- Pro Portfolio: Wert, Einstandsbasis, unrealisierter/realisierter/gesamter G/V, bestes & schlechtestes Asset, **Allokation** nach Asset oder Klasse und ein **Wertverlauf-Chart** (Tag/Woche/Monat/Jahr), der sich füllt, während Aktualisierungen sich ansammeln.
- Die **Beschreibung** bleibt nach der Erstellung bearbeitbar, neben einer einklappbaren Notiz zur **Anlagethese**, die beim Portfolio bleibt, dafür, warum du hältst, was du hältst.

### Cash, Erträge und Kosten

Das Ledger besteht nicht nur aus Käufen und Verkäufen. **Cash & Erträge** im Tab Operationen bucht eine
**Einzahlung**, eine **Auszahlung**, eine **Dividende**, **Zinsen**, einen **Kupon**, eine **Gebühr** oder eine
**Steuer**, jeweils in eigener Währung und mit optionaler einbehaltener Gebühr. Ein Ertrag kann den
Bestand nennen, der ihn gezahlt hat, oder gar keinen, wenn er vom Konto selbst kam.

Aus diesen Zeilen erhält das Portfolio einen Cash-Saldo pro Währung, einen Gesamtertrag und ein echtes
Nettovermögen (Positionen plus Cash). Negatives Cash wird angezeigt, nie gekappt: Es bedeutet Margin oder ein
Ledger, dem seine Einzahlungen fehlen, und beides ist sehenswert.

### Analyse

Der Tab **Analyse** beantwortet Performance, Risiko und Engagement an einem Ort. Sieben unabhängige
Ansichten, jede fragt nur nach den Daten, die sie braucht, sodass *Buch* auf einer frischen Installation sofort rendert,
während *Stress* für Candles zahlt.

Eine Ansicht, die nicht antworten kann, **sagt warum und was zu tun ist**. Sie zeigt nie eine Null, die sie nicht
gemessen hat. Noch keine Historie, ein zu kurzes Buch zum Annualisieren, kein Benchmark gewählt, keine Candles
dafür, keine Zielwerte gesetzt: Jedes davon ist ein Satz und eine Schaltfläche, kein leerer Chart.

| Ansicht | Braucht | Beantwortet |
|---|---|---|
| **Buch** | das Ledger, sonst nichts | Nettovermögen, investiert gegenüber Cash, unrealisiert, realisiert, Erträge, Allokation nach Klasse |
| **Performance** | tägliche Historie | Rendite, IRR, annualisiert, Netto-Einzahlungen, pro Fenster |
| **Risiko** | tägliche Historie | Volatilität, Drawdown, Sharpe, Sortino, Calmar, beste und schlechteste Perioden |
| **Benchmark** | tägliche Historie und die Candles des Benchmarks | was der Index bei deiner Volatilität zurückgebracht hätte, Alpha, Beta, Capture |
| **Erträge & Kosten** | die Cash-Zeilen des Ledgers | vereinnahmte Erträge, gezahlte Kosten, jährlicher Drag, die Kurve ohne Gebühren |
| **Allokation** | eine Zielallokation | aktuell gegenüber Ziel, Drift, die Trades, die sie schließen |
| **Stress** | tägliche Candles pro Bestand | historische Wiedergabe und Faktorschocks, mit dem abgedeckten Anteil |

Jede Ansicht läuft über dieselbe Fensterauswahl: 1M, 3M, 6M, YTD, 1J, 3J, 5J, alles oder ein eigener
Bereich. Ein Fenster, das länger ist als deine Historie, wird als **nicht abgedeckt** gemeldet, mit den Tagen, die
tatsächlich vorliegen, statt als volle drei Jahre ausgegeben zu werden.

### Performance und Risiko

**Performance** meldet eine zeitgewichtete Rendite neben einem IRR, und sie beantworten unterschiedliche
Fragen. Zeitgewichtet ist, was die Investments getan haben, einzahlungsbereinigt, denn eine Einzahlung
ist keine Rally. IRR ist, was **du** bekommen hast, kapitalgewichtet, ein gutes Timing deiner Käufe zeigt sich also
dort und nirgendwo sonst. Netto-Einzahlungen stehen daneben, und eine Rendite unter zwei Monaten wird
nicht annualisiert: Sechs Wochen mit acht zu multiplizieren ist eine Prognose, keine Messung.

**Risiko** liest dieselbe Kurve: Volatilität, maximaler Drawdown, Sharpe, Sortino, Calmar, der Anteil der
Tage, die im Plus endeten, bester und schlechtester Tag, Monat, Quartal und Jahr, und jeder Drawdown, der tiefer
als 2 % ist, mit **wie lange es dauerte, ihn zu füllen**. Einer, der noch offen ist, ist als laufend markiert, mit dem Abstand
zum letzten Hoch und der Zahl der Tage seither.

Der Annualisierungsfaktor wird **an deiner eigenen Kurve gemessen**, nicht angenommen. Ein Aktienbuch handelt
etwa 252 Tage im Jahr und ein Krypto-Buch 365, und ein gemischtes ist keins von beiden. Sharpe und
Sortino nutzen den risikofreien Zins aus den Messeinstellungen.

### Benchmark

Wähle ein Instrument, dessen Tageskerzen du hast (SPY, QQQ, BTCUSDT), und die Seite beantwortet die
einzige Frage, die einen Streit entscheidet: **was dieser Index bei deiner Volatilität zurückgebracht hätte**, neben
dem, was du tatsächlich verdient hast. Den Index zu schlagen, indem man das Dreifache seines Risikos eingeht,
ist kein Schlagen.

Darunter: Gesamt- und annualisierte Rendite für beide, Volatilität, maximaler Drawdown und Sharpe nebeneinander,
dann Alpha, Beta, Tracking Error, Information Ratio sowie Up- und Down-Capture.

Dein Buch wird über die **eigenen Sitzungen des Benchmarks** gemessen. Vergleichst du ein 24/7-Portfolio mit einem
Index Tag für Tag, frisst jeder Montag des Index ein Wochenende von dir, was deine Rendite
stillschweigend zu niedrig ausweist.

### Erträge und Kosten

Was du vereinnahmt hast, was du gezahlt hast und was das Zahlen dich gekostet hat. Dividenden, Zinsen und
Kupons auf der einen Seite; Handelsgebühren und Kontogebühren auf der anderen, mit dem jährlichen Drag als
Anteil deines durchschnittlichen Nettovermögens.

Die Kurve wird zweimal gezeichnet: wie es geschah, und dasselbe Buch mit entfernten Gebührenbeinen. Gebühren
stecken bereits in deiner Einstandsbasis und deinem Cash, das ist also ein Vergleich, keine Subtraktion,
die du selbst machen könntest.

### Zielallokation

Lege unter **Ziele setzen** fest, welchen Anteil des Nettovermögens jeder Bucket halten soll und wie weit er driften darf, bevor er
als abweichend gilt. Eine Allokation muss sich auf 100 % addieren, und einem Bucket kann per Klick der Rest
zugewiesen werden. Das Speichern einer leeren Liste schaltet die Ansicht aus.

Die Ansicht zeigt dann aktuell gegenüber Ziel pro Bucket, die Abweichung, ob jeder in seinem Band liegt, und die **Trades, die die Lücke schließen würden**: kaufe so viel von dem, verkaufe so viel von jenem. Alles, was du ohne Ziel hältst, wird aufgelistet statt ignoriert.

Nur Anzeige. Nichts hier gibt eine Order auf, und nichts rebalanciert von selbst.

### Stresstests

Zwei Engines, und beide sagen dir, wie viel deines Buchs die Zahl abdeckt.

**Historische Wiedergabe** wendet den realisierten Tagesverlauf von 2008, 2020, 2022, 2018 Q4 oder des Krypto-Hochs von 2021
auf das an, was du heute hältst, mit den eigenen Candles der Instrumente über diese Daten. Kein
Modell, kein Proxy. Ein Instrument, das es damals nicht gab, hat keinen Verlauf: Es wird **benannt und
ausgeschlossen**, nie durch einen Index ersetzt.

**Faktorschock** bewegt ein reales Instrument (S&P 500, Nasdaq, Zinsen, EUR/USD, Öl, Credit-
Spreads) und erreicht jeden Bestand über eine **gemessene** Sensitivität, an seinen eigenen Candles gefittet. Ein Bestand ohne Candles, mit zu kurzer Historie oder einem Fit ohne Erklärungskraft
bekommt **kein Beta**: Er landet mit seinem Gewicht in *unerklärt*, und die Schlagzeile lautet
„−11,8 % über die 74 % des Buchs, die gemessen werden konnten“. Cash hat ein Beta von null, was
meist die einzige bereits vorhandene Diversifikation ist.

Ein Zinsschock in Basispunkten erreicht eine Anleihe über eine im Szenario festgehaltene Duration, damit sich über
die Zahl streiten lässt. Rezession, Inflationsspitze und Credit-Ausweitung werden als bearbeitbare
Kombinationen dieser Beine ausgeliefert.

Ein Bereitschaftspanel listet, was gestresst werden kann und was nicht, bevor du etwas ausführst, damit
ein dünnes Ergebnis vorher erklärt wird statt hinterher.

### Tägliche Historie

Jedes Maß oben außer *Buch* braucht eine Kurve, und Snapshots beginnen erst an dem Tag, an dem du den
täglichen Job einschaltest. Ein Portfolio, das du seit sechs Jahren führst, würde sonst ab
letztem Dienstag gemessen. Die Kurve wird daher Tag für Tag **aus dem Ledger und den gespeicherten Candles neu aufgebaut**.

Das Zahnrad im Tab Analyse öffnet die **Mess-Einrichtung**:

1. **Benenne den Candle-Ticker jedes Assets** und die Währung, in der diese Candles notiert sind. Ein Ticker
   in deinem Ledger ist nicht immer das Symbol, das dein Anbieter bedient, und eine in EUR gekaufte Aktie,
   bepreist gegen USD-Candles, liegt um den Wechselkurs daneben.
2. **Fehlende Candles herunterladen**. Sie werden als gewöhnliche [Historical Data](/de/modules/market-data#histdata)-Jobs
   über die Konnektoren eingereiht, die Portfolios gewährt sind. Trägt keiner von ihnen eines deiner
   Instrumente, wird es genannt, mit dem, was zu gewähren ist.
3. **Die Kurve neu aufbauen**. Es meldet die neu aufgebauten Tage und die Tage, die übersprungen wurden, weil ein Bestand
   an diesem Tag keine Candle hatte. Ein Tag, der nicht bewertet werden kann, wird nicht gespeichert, statt falsch gespeichert zu werden.

Das Bearbeiten einer in der Vergangenheit datierten Operation markiert die Kurve **ab diesem Datum als veraltet** und sagt das.
Das Neuaufbauen bleibt deine Entscheidung. Das Aktualisieren eines Portfolios lädt auch Fehlendes herunter und verlängert
die Kurve dahinter und sagt dir, wenn ein Broker eines deiner Instrumente nicht bedienen kann.

### Ein Operations-Ledger importieren

**Import** in der Portfolio-Kopfzeile liest einen Broker-Export, eine Tabelle oder einen anderen Tracker (CSV, TSV, JSON, Parquet) und macht aus jeder Zeile eine Kauf- oder Verkaufsoperation. Dieselbe Erkennungs-Engine wie beim [Journal-Import](/de/modules/journal#import-a-trade-book): Header in sechs Sprachen plus Wert-Sniffing, Trennzeichen-, Dezimal- und Datumskonventionen pro Spalte entschieden, nicht erkannte Spalten bleiben unzugeordnet.

Drei Dinge, die es nicht rät:

- **Was ein Symbol ist.** Jedes Symbol in der Datei muss auf ein Asset zeigen: eines, das du in diesem Portfolio bereits hältst (automatisch zugeordnet), ein neu anzulegendes Asset oder *überspringen*. Ein ungelöstes Symbol blockiert den Import, und Assets werden erst nach deiner Bestätigung angelegt.
- **Eine Zeile, die weder Kauf noch Verkauf noch eine erkannte Art ist**, wird als Zeilenfehler aufgelistet, statt zu einer Operation erfunden zu werden. Dividenden, Einzahlungen, Auszahlungen, Gebühren und Steuern **werden** in sechs Sprachen erkannt und als solche gebucht. Nennt die Datei gar keine Art, setze den Standard einmal für den ganzen Import.
- **Ein fehlender Preis** wird aus Betrag ÷ Menge abgeleitet und markiert, nie stillschweigend aufgefüllt.

Nichts wird geschrieben, bevor du die Vorschau bestätigst. Jeder Import ist ein **Batch**, als Ganzes rückgängig zu machen (angelegte Assets bleiben), und wird **pro Portfolio** dedupliziert, ein erneuter Import derselben Datei ändert also nichts.

### Bestände von einem Broker importieren

**Vom Broker** in der Portfolio-Kopfzeile liest die Bilanz eines [Broker-Kontos](/de/config/brokers) statt einer Datei: Der Unterschied zu deinem Ledger wird Zeile für Zeile gezeigt, und jede Zeile, die du annimmst, schreibt die eine Operation, die das Portfolio in Einklang bringt. Die Einstandsbasis wird genutzt, wo der Broker eine veröffentlicht (Interactive Brokers), und erfragt, wo nicht (die Krypto-Börsen veröffentlichen eine Menge und sonst nichts).

## MyWealth {#wealth}

Nettovermögen über **alles**: Brokerkonten, Immobilien, Krypto, Cash, Wertgegenstände. Wo der Portfolio Tracker live bepreiste Bestände verfolgt, verfolgt MyWealth jedes Asset, das du selbst bewertest.

- Füge Assets mit Name, Typ, Währung und Kategorie hinzu, dann **erfasse Wertaktualisierungen** über die Zeit (Preis × Menge oder ein direkter Wert, mit einer Notiz). Die Historie ist bearbeitbar.
- **Nettovermögen-Chart** nach Monat oder Jahr, plus eine Aufschlüsselung pro Kategorie. Mehrwährung mit derselben FX-Behandlung wie im Journal (Assets ohne Kurs werden ausgeschlossen und markiert).
- **Vorlagen**, wie im Journal: reservierte Preis-/Mengenfelder speisen den Wert, eigene Felder halten Notizen pro Revision.
- **Besessen oder geschuldet**: Ein Asset kann eine **Verbindlichkeit** sein (eine Hypothek, ein Kredit), sodass die Schlagzeile ein echtes Nettovermögen ist. Die Seite zeigt, was du besitzt und was du schuldest, bevor sie beides saldiert.
- **Ein Portfolio verknüpfen**, statt es zu kopieren: Ein verknüpftes Portfolio wird jedes Mal live aus dem Tracker gelesen, sein Wert in deinem Nettovermögen ist also nie eine veraltete Kopie.
- **Alter der Bewertung**: Sage einem Asset, wie oft es neu bewertet werden soll, und die Seite nennt die, die darüber hinaus gealtert sind, die ältesten zuerst. Ein vor drei Jahren bewertetes Haus ist stillschweigend falsch, und das hier bricht das Schweigen. Ein verknüpftes Portfolio veraltet nie: Es wird gelesen, nicht gemerkt.

## Managers' Portfolios {#mportfolios}

Durchstöbere die **13F-Portfolios von Superinvestoren**: was bekannte Fondsmanager halten, Positionsgrößen, jüngste Aktivität, gemeldeter gegenüber aktuellem Wert, 52-Wochen-Spannen. Filtere nach Manager oder nach Ticker (*wer hält AAPL?*).

Da sich 13F-Daten vierteljährlich ändern, kannst du **Snapshots** jedes Portfolios speichern und über die Zeit vergleichen.

## Tax Calculator {#taxcalc}

Grobe Schätzung von Trading- und Anlagesteuern. **Keine Steuerberatung.**

- **Profile** starten aus **Länder-Vorlagen** (privat oder professionell) und bleiben vollständig bearbeitbar: Grenzsteuersatz, Sozialabgaben, Freibeträge für Kursgewinne und Dividenden, optionale **Vermögensteuer-Stufen** (z. B. CH, ES, NO), Stufen für langfristige Entlastung.
- Gib Zahlen im Modus **Zusammenfassung** (Start-/Endwert, Einzahlungen, Auszahlungen, realisierter Anteil) oder im Modus **Einzelposten** (Kapitalgewinne, Derivategewinne, Kryptogewinne, Dividenden, Zinsen, vorgetragene Verluste) ein.
- **Trading Journal laden**: Ist das Journal installiert, lädt ein Klick den realisierten PnL eines Steuerjahrs, aufgeteilt in Kapital-/Derivate-/Kryptogewinne, zum FX-Kurs am Jahresende umgerechnet.
- **Vom Broker**: Ist dem Tax Calculator ein [Broker-Konto](/de/config/brokers) gewährt, wird ein Steuerjahr direkt aus dem Konto gelesen, zu geschlossenen Positionen gefaltet und pro Formularzeile summiert, jede Veräußerung zum Kurs ihres eigenen Ausstiegsdatums umgerechnet.
- Ergebnisse zeigen die geschätzte Steuer mit einer Aufschlüsselung pro Posten (steuerpflichtig, Freibetrag, Basis, Satz) und den effektiven Satz. Speichere Szenarien im Verlauf, um zu vergleichen.

## Subscriptions {#subscriptions}

Jede wiederkehrende Ausgabe in einer Liste (Trading-Werkzeuge, Datenfeeds, Streaming) mit Preis, Währung, Abrechnungsfrequenz (wöchentlich/monatlich/vierteljährlich/jährlich) und Kategorie.

Du erhältst monatliche/jährliche **Ausgaben-Charts** (gruppiert oder pro Abo), das **Monatsäquivalent** jedes Abos, nächste Abrechnungsdaten und Summen für den nächsten Monat. Pausiere ein Abo, um es gelistet zu lassen, ohne es mitzuzählen.
