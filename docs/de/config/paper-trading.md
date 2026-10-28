# Papierhandel

Ein **Papierhandelskonto** ist ein Broker, den OpenTraderWorld selbst führt. Orders an dieses Konto werden zum Live-Preis des Charts ausgeführt, aus dem sie kommen, gegen Ihr eigenes simuliertes Guthaben, mit den Provisionen, dem Slippage und dem Hebel, die Sie festlegen. Kein Schlüssel, kein Handelsplatz, kein echtes Geld.

Es wird aus dem Chart genau wie ein echter Broker gehandelt: dasselbe Ticket, dieselbe Schnellleiste, dieselben Order- und Positionslinien, Panel-Tabs und dasselbe Audit-Log. Diese sind unter [Handel aus dem Chart](/de/config/trading) beschrieben; diese Seite behandelt, was nur für Papier gilt.

## Eines anlegen

**Einstellungen → Broker → Konto hinzufügen**, Broker **Paper trading**. Keine Zugangsdaten. Die Einstellungen (in der App auf Englisch beschriftet):

| Einstellung | Standard | Bedeutung |
|---|---|---|
| **Account currency** | `USD` | Währung von Guthaben und Ergebnissen, 3 bis 5 Buchstaben |
| **Starting balance** | `100000` | Als erste Einzahlung gebucht |
| **Commission per order** | 0 | Ein fester Betrag pro Ausführung |
| **Commission per unit** | 0 | Pro Aktie oder Kontrakt |
| **Commission (% of value)** | 0 | `0.1` bedeutet 0,1 % des gehandelten Werts |
| **Minimum commission** | 0 | Eine Ausführung kostet nie weniger |
| **Slippage (basis points)** | 0 | `5` bedeutet 0,05 %, zu Ihren Ungunsten |
| **Leverage** | 1 | 1 ist ein Cash-Konto |
| **Short selling** | yes | `no` lehnt einen Verkauf ohne Bestand ab |
| **Fill on** | `quote` | `quote` handelt zu Bid und Ask, `last` zum letzten Kurs |

Zahlen müssen null oder größer sein, der Hebel mindestens 1. Die Einstellungen werden bei jeder Ausführung neu gelesen, eine Änderung gilt also ab der nächsten.

Ein neues Papierkonto hat **Alle Module** und **Trading** eingeschaltet. Trading auf einem Papierkonto fragt nach keinem Passwort und liegt nie hinter der [Trading-Sperre](/de/config/trading#trading-lock).

**Verbindung testen** antwortet mit Equity, Guthaben, offenen Positionen und aktiven Orders des Kontos.

## Guthaben

Das Panel **Papierkonto** unter dem Konto in Einstellungen → Broker hat ein Feld **Betrag** und drei Buttons:

- **Einzahlen** fügt den Betrag dem Guthaben hinzu.
- **Auszahlen** entnimmt ihn, höchstens bis zum gehaltenen Guthaben (nicht der Equity).
- **Konto zurücksetzen** entfernt jede Order, Ausführung und Guthabenbewegung und beginnt dann mit einer einzigen Einzahlung neu: dem eingegebenen Betrag oder dem Startguthaben, wenn das Feld leer ist. Die Einstellungen bleiben, und bereits ins Journal importierte Trades bleiben erhalten.

Das Panel zeigt außerdem die Summe der Einzahlungen und die letzten 50 nicht mehr aktiven Orders mit Status (ausgeführt, storniert, abgelehnt, abgelaufen) und Grund. **In dieser Liste wird eine Ablehnung erklärt.**

## Wie Orders ausgeführt werden

### Zu welchem Preis

Eine Order wird zum **Live-Feed des Daten-Connectors des Charts** ausgeführt: dem Connector, von dem die Order kam, oder dem für Charts freigegebenen Connector des Anbieters. Die Engine läuft auf dem Server, **Orders werden also auch bei geschlossenem Chart ausgeführt**, und ruhen, solange die App aus ist.

- Ein Instrument ohne Live-Feed wird beim Platzieren abgelehnt. Ein Index ebenfalls: Es gibt keinen Markt zum Handeln.
- Die Engine verfolgt die feinsten Live-Bars des Anbieters (1 Minute, wenn verfügbar) und seine Quotes, wenn der Feed sie veröffentlicht. Nur Preise nach dem Aktivwerden der Order zählen.
- Mit **Fill on: quote** werden Käufe zum Ask und Verkäufe zum Bid ausgeführt, solange das Quote jünger als 15 Sekunden ist. Sonst, und mit **last**, beide zum letzten Kurs.
- Ein Bar älter als 2 Minuten wird nicht genutzt. Eine Market-Order bei geschlossenem Markt wartet auf den ersten frischen Preis, statt abgelehnt zu werden.
- Stoppt der Feed, wird er alle 10 Sekunden neu versucht, und die Orders warten. Wird der Daten-Connector gelöscht, können seine Orders nicht mehr ausgeführt werden.

### Nach Ordertyp

| Typ | Ausgeführt wenn | Zu |
|---|---|---|
| **Market** | beim nächsten Preis | diesem Preis, plus Slippage |
| **Limit** | der Markt das Limit erreicht | dem Limit, oder besser, wenn der Markt schon darüber hinaus ist. Kein Slippage |
| **Stop** | der Markt den Stop erreicht | dem Stop bei Berührung, dem Markt bei einer Lücke, plus Slippage |
| **Stop limit** | der Stop erreicht ist, dann das Limit | wie ein Limit, eventuell zum selben Preis |
| **Trailing stop** | der Markt kommt um den Abstand von seinem besten Preis seit dem Start des Stops zurück (der höchste hinter einem Verkauf, der niedrigste hinter einem Kauf) | wie ein Stop |
| **MIT** | der Markt erreicht den Trigger von der Limit-Seite her | dem Markt, plus Slippage |
| **LIT** | der Trigger, dann das Limit | wie ein Limit |

- **Keine Teilausführungen**: Eine Order wird vollständig ausgeführt, ohne Volumen- oder Liquiditätsprüfung.
- Ein **Trailing stop** mit Startpreis wartet, bis der Markt ihn erreicht (hinter einem Verkauf steigt er darauf, hinter einem Kauf fällt er darauf), und folgt dann ab diesem Preis. Sein bester Preis bleibt über einen Neustart erhalten.
- **Reduce only**: allein platziert, wird sie abgelehnt, außer das Konto hält eine Position auf der Gegenseite. Bei der Ausführung wird sie auf das begrenzt, was dann gehalten wird, und storniert, wenn nichts mehr zu verkleinern bleibt.
- **Post only**: ein Limit, das bei der Platzierung abgelehnt wird, wenn es sofort gegen den aktuellen Bid oder Ask ausgeführt würde (den letzten Preis ohne Quote).
- Kein Iceberg, keine Trigger-Wahl, kein Outside-RTH, kein Close-position und kein Algo: Ein Live-Preis ohne Tiefe, Handelszeit oder Mark hat nichts, worauf sie sich abspielen ließen, daher bietet das Ticket sie nicht an.
- Der Ausführungspreis wird auf den Tick des Instruments gerundet, wenn er bekannt ist.
- **Provision** pro Ausführung = pro Order + pro Einheit × Menge + % × gehandelter Wert, angehoben auf das Minimum. Sie wird in der Währung des Instruments belastet.

### Gültigkeit

- **Day** (Standard) läuft um die nächste **Mitternacht UTC** ab, nicht zum Börsenschluss.
- **GTC** läuft nie ab. **GTD** läuft zum angegebenen Datum und zur angegebenen Uhrzeit ab.
- **IOC** und **FOK** werden storniert, wenn der erste Preis, auf den sie treffen, sie nicht ausführt. Bei geschlossenem Markt warten sie auf diesen ersten Preis. Paper führt vollständig aus, daher enden beide gleich.

Der Ablauf wird etwa alle 10 Sekunden geprüft; die Order erscheint dann als abgelaufen.

### Brackets und OCO

- Ein **Take profit** oder **Stop loss** wartet auf die Ausführung seines Einstiegs und wird dann aktiv. Wird der Einstieg storniert, abgelehnt oder läuft ab, werden seine Ausstiege storniert.
- Bei einer **OCO** storniert eine Ausführung die andere Order der Gruppe und deren Ausstiege.
- Preise, Menge und Gültigkeit einer aktiven Order lassen sich ändern. **Position schließen** sendet eine GTC-Market-Order und storniert dann die Ausstiege der Position.

## Mittel und Hebel

- Eine Position bindet ihren Wert ÷ Hebel. Futures binden stattdessen ihre Margin pro Kontrakt (siehe unten) und ignorieren den Hebel.
- Eine Order, die eine Position eröffnet oder vergrößert, wird beim Platzieren und erneut zum Ausführungspreis geprüft, Provision eingeschlossen. Sie wird abgelehnt, wenn das Konto mehr als seine Equity binden würde:

  > not enough funds: the positions would tie up X USD and the account's equity is Y USD

- Eine Order, die eine Position verkleinert, ist immer erlaubt.
- Mit **Short selling** auf `no` wird ein Verkauf, der short gehen würde, abgelehnt.
- Es gibt **keinen Margin Call und keine Zwangsliquidation**: Die Equity kann unter die genutzte Margin fallen.

## Futures

Ein Future braucht seine Kontraktdaten in den **Instrumenteinstellungen** des Charts:

- **Tickgröße, Punktwert und Währung.** Sind sie leer, werden sie aus den Open-Source-Kontraktspezifikationen von [QuantConnect Lean](https://github.com/QuantConnect/Lean) gefüllt, die in die App eingebettet sind und wöchentlich aktualisiert werden. Der Dialog nennt die gefüllten Felder; speichern Sie, um sie zu behalten, oder geben Sie eigene ein. Ihre gespeicherten Werte haben immer Vorrang.
- **Margin pro Kontrakt (Papier)** in der Währung des Instruments: Nehmen Sie die Initial Margin Ihres Brokers. Oder haken Sie **Kontrakt stattdessen zum vollen Wert zählen** an: Menge × Preis × Punktwert.

Eine Order auf einen Future ohne Margin wird abgelehnt, und der Chart bietet **Margin festlegen** (öffnet die Instrumenteinstellungen) oder **Vollen Wert verwenden** an. Beides sendet die Order erneut.

Ein Ticker wird auf seine Kontraktwurzel reduziert (`MNQZ6`, `ESZ25`, `@ES`, `ES=F`, `/ES`). Eine an mehreren Börsen gelistete Wurzel ohne Angabe der Börse ist ein Fehler, der die Kandidaten nennt: **Der Markt wird nie geraten.** Ein Future oder eine Option ohne Punktwert wird abgelehnt, ebenso jedes Instrument ohne Währung, außer einem Forex-Paar, dessen Währung aus dem Namen gelesen wird (`EURUSD` ist in USD).

## Kontokennzahlen

Der Tab **Konto** des Order-Panels und das Einstellungs-Panel zeigen:

| Kennzahl | Berechnung |
|---|---|
| **Cash** | Einzahlungen − Auszahlungen + Realisiert − Provisionen |
| **Offenes Ergebnis** | Σ (Kurs − Durchschnittspreis) × Menge × Punktwert |
| **Equity** | Cash + offenes Ergebnis |
| **Realisiert** | seit dem letzten Zurücksetzen, vor Provisionen, auf Durchschnittskostenbasis |
| **Provisionen** | Summe der Gebühren |
| **Initial Margin** | Margin pro Kontrakt der Futures + Wert der übrigen Positionen ÷ Hebel |
| **Verfügbar** | Equity − Initial Margin, nie unter null |
| **Positionswert** | Bruttoexposure, long und short |

Der Tab aktualisiert sich alle 5 Sekunden, solange er offen ist.

### Andere Währungen

Das Konto hat eine einzige Währung. Ein Instrument in einer anderen wird mit dem **Tageskurs aus dem Journal** umgerechnet: realisierte Ergebnisse und Gebühren zum Kurs ihrer Ausführung, offene Ergebnisse und Margin zum heutigen.

- Eine Order wird abgelehnt, wenn kein Kurs existiert: `no EUR to USD rate: add one in the journal's FX settings, or trade in USD`.
- Eine Position, deren Kurs später fehlt, wird als aus den Summen ausgeschlossen gelistet.

## Ins Journal

Papierausführungen werden wie die jedes Brokers importiert: **Journal → Import → Von einem Broker**, Papierkonto, Buch und Zeitraum wählen, Vorschau, importieren. Siehe [Trades ins Journal importieren](/de/config/brokers#import-trades-into-the-journal). **Automatisch synchronisieren** funktioniert ebenfalls.

## Gut zu wissen

- Beträge werden als exakte Dezimalzahlen geführt, wie eine Börse sie bucht: 0,1 dreimal gekauft und 0,3 verkauft lässt nichts offen, und ein Preis muss genau auf dem Tick des Instruments liegen.
- Guthaben, Orders und Ausführungen werden in der Datenbank gespeichert und überstehen Neustarts. Live-Preise nicht: Nach einem Neustart werden Positionen bis zum nächsten Kurs zu ihrem Durchschnittspreis bewertet.
- Das Löschen des Kontos löscht sein Guthaben, seine Orders und Ausführungen.
- Jeder Order-Aufruf steht im [Audit-Log](/de/config/trading#audit-log).
- Nicht modelliert: Warteschlangenposition, Liquidität, Teilausführungen, Zinsen, Leihgebühren, Funding, Dividenden.
- Papier-Orders und Guthabenbewegungen sind für [KI-Agenten (MCP)](/de/config/ai-agents) nicht freigegeben, und Papierkonten sind im [Demo-Modus](/de/guide/demo) deaktiviert.
