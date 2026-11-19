# Trading-Bots {#trading-robots}

Ein **Trading-Bot** ist eine Backtest-Strategie, die vorwärts läuft. Er friert die Einstellungen der Strategie ein, läuft bei jedem Schluss des Timeframes der Strategie auf einer Reihe von Instrumenten und simuliert entweder seine Fills oder sendet seine Orders an ein Broker-Konto. Über der Strategie hat jeder Bot sein eigenes Geld (eine **Kapitalregel**), **Ausschlüsse**, **Schutzregeln**, die auf seine eigenen Ergebnisse reagieren, und eine **Priorität**.

Es gibt keine zweite Engine: Jeder Lauf liest das Fenster mit dem gewöhnlichen Backtest und handelt nur auf der gerade geschlossenen Candle. Ein Signal-Bot führt die Positionen, die er eingegangen ist, so fort, wie er sie eingegangen ist; was das Fenster auf früheren Candles zeigt, ist Historie, nie erfasst oder gesendet.

Die Strategie liefert die Signale, ihren Stop und ihr Ziel sowie ihre Ordertypen. Der Bot liefert das Geld und den Kalender: seine [Kapitalregel](#capital-policy) und seine [Ausschlüsse](#exclusions). Das Instrument (Punktwert, Preis-Tick, Größenstufe, Minimum) ist das des Handelsplatzes. Kapital, Größe, Tages- und Sitzungsfilter sowie Kosten der Strategie werden nicht gelesen.

Das Modul braucht **Backtest** installiert. Es liegt unter **/robots**, als **Trading-Bots** im Modulwechsler.

## Die Flotte {#the-fleet}

Die Flottenseite listet jeden Bot: Karten auf dem Handy, eine sortierbare Tabelle auf dem Desktop. Sie aktualisiert sich alle 10 Sekunden, solange die Seite sichtbar ist, und die Countdowns laufen jede Sekunde weiter.

- **Kennzahlen**: **Bots** (mit der Zahl der aktiven), **Kapital** (aufgeteilt nach Bot), **Heute** (der GuV des Tages und die ungelesenen Ereignisse), **Offen** (offene Positionen und wie viele Bots eine halten).
- **Flotten-GuV**: die summierte Equity-Kurve der Bots, mit **GuV**, **Realisiert** und **Unrealisiert**. Filtere sie nach Umgebung (**Alle Umgebungen**, Simuliert, Papier, Testnet, Live) und nach einem Datum **Von** und **Bis**. Ohne Enddatum wird der letzte Punkt zu den offenen Positionen bewertet.
- **Rangliste**: die Bots nach der GuV ihrer geschlossenen Trades **Heute**, über **1W**, **1M** oder **Alle**, mit der Rendite auf ihr Startkapital.
- **Exposure nach Anlageklasse**: was die offenen Positionen zum letzten Kurs einsetzen, nach Anlageklasse aufgeteilt (Krypto, Futures, Forex, Aktien...), mit der Zahl der Bots, die je Klasse eine halten.
- **Statusfilter**: Ein Klick auf einen Chip zeigt die Bot in diesem Status (**Aktiv**, **Von Schutzregel gehalten**, **Gestoppt**, **Entschärft, erneut scharf schalten**, **Beeinträchtigt**, **Blockiert**, **Broker nicht erreichbar**, **Fehler**), jeder mit einer Anzahl. Ein zweiter Klick hebt ihn auf. Die Kontoauswahl grenzt auf **Simuliert** oder ein Konto ein.
- **Braucht Aufmerksamkeit**: die Bots mit einem Fehler, einer haltenden Schutzregel oder ungelesenen Ereignissen, jeder mit dem Grund.
- **Nächste Ticks**: die acht Bots, die als Nächstes fällig sind, mit ihren Instrumenten und einem Countdown.
- **N als gelesen markieren** leert die ungelesenen Ereignisse jedes Bots. **Neuer Bot** öffnet den Assistenten.

## Einen Bot anlegen {#create-a-robot}

**Neuer Bot** auf der Flottenseite, oder **Als Bot starten** im Backtest: bei einem Ergebnis (der Lauf genau so, wie er lief) oder bei einer gespeicherten Strategie in der Bibliothek. In beiden Fällen öffnet sich der Assistent, ein Schritt nach dem anderen:

1. **Strategie**: der Name des Bots und eine gespeicherte Strategie zum Einfrieren (oder der Lauf, aus dem er stammt). **Strategie erstellen** öffnet den Backtest. Eine spätere Bearbeitung der Strategie ändert nichts am Bot, bis du ihn aktualisierst (siehe [Einstellungen](#settings)).
2. **Instrumente**: **Instrument hinzufügen** öffnet die Datenauswahl des Charts, die die Symbolsuche jedes Anbieters durchsucht. Keine gespeicherte Historie ist nötig: Jeder Lauf holt die Candles, die die Strategie liest. Bis zu 8 Instrumente, auf einer gemeinsamen **Zeiteinheit**. Jedes Instrument wird über die Datenquelle gelesen, aus der es gewählt wurde, was sich später in den **Einstellungen des Bots** ändern lässt. Wo der Anbieter einen Live-Preis streamt, lösen Stops, Ziele und Limits darauf aus; sonst werden sie bei Candle-Schluss geprüft. Die Einstellungen des Bots sagen, was für jedes Instrument gilt.
3. **Konto**: **Simulieren** (nichts wird irgendwohin gesendet) oder **Broker-Konto** (siehe [Auf einem Broker-Konto](#on-a-broker-account)).
4. **Kapital**: die [Kapitalregel](#capital-policy).
5. **Schutzregeln**: die [Schutzregeln](#guards). Ein neuer Bot startet mit dem Standardsatz.
6. **Zeitplan und Benachrichtigungen**: Der Bot läuft bei jedem Schluss seiner Candles und entscheidet auf der gerade geschlossenen Candle. Der Schritt enthält seine [**Ausschlüsse**](#exclusions), einen optionalen **Statusbericht** (**Statusbericht senden**, dann wann er rausgeht: alle N Minuten, täglich, wöchentlich, monatlich oder einmalig, in einer Zeitzone), **Gleitende Kerzen** (wie viel Historie jeder Lauf der Engine gibt, standardmäßig 5000), **Priorität**, **Benachrichtigen** (**Jeder Lauf**, **Nur bei neuer Ausführung**, **Nie**) und **Kanäle**.
7. **Prüfen**, dann **Bot anlegen**.

Der Server prüft beim Anlegen des Bots alles erneut. Eine Ablehnung wird so gezeigt, wie sie formuliert wurde, und nennt, was zu ändern ist.

Der Bot startet sofort. Das Fenster **Name des Bots wird gestartet** begleitet seinen ersten Lauf, während er seine Historie herunterlädt und sein Buch kennenlernt; **Im Hintergrund weiter** schließt es und öffnet die Seite des Bots, während der Lauf weitergeht. Sobald er bereit ist, läuft ein simulierter Bot ab da bei jedem Schluss, und ein Broker-Bot bietet im selben Fenster **Scharf schalten** an (siehe [Auf einem Broker-Konto](#on-a-broker-account)). **Bot öffnen** öffnet seine Seite.

Kanäle sind die Benachrichtigungskanäle, die den Trading-Bots gewährt wurden; die Kanal-Schaltfläche des Schritts fügt sie hinzu oder verwaltet sie, ohne den Assistenten zu verlassen. Ohne gewählten Kanal sendet der Bot an jeden den Trading-Bots gewährten Kanal; ist keiner gewährt, wird nichts gesendet, und jedes Ereignis wird trotzdem im Protokoll des Bots erfasst. **Benachrichtigen** auf **Nie** sendet gar nichts. Der Statusbericht gibt den Zustand des Bots, seine Zahlen seit dem letzten Bericht und was er hält; er wird nicht gesendet, solange der Bot ausgeschlossen ist und nichts hält. Die **Einstellungen des Bots** enthalten außerdem Meldungen bei Einstiegen und Ausstiegen je Instrument und deinen eigenen Wortlaut für die Einstiegs- und Ausstiegsnachrichten sowie für die Zusammenfassung, wenn ein Statusbericht eingeschaltet ist.

### Ausschlüsse {#exclusions}

Ausschlüsse sind Zeiträume, in denen der Bot keinen neuen Einstieg nimmt. Ausstiege werden auch darin verwaltet. Sie werden in der Zeitzone des Bots gelesen, und jede Mischung funktioniert:

- **Ganze Tage**: die gewählten Wochentage.
- **Uhrzeitbereiche**: von einer Uhrzeit bis zu einer anderen, das Ende ausgeschlossen; ein Bereich, der endet, bevor er beginnt, läuft über Mitternacht.
- **Datumsbereiche**: von einem Datum bis zu einem anderen, beide Tage eingeschlossen.

Bis zu 20 Uhrzeitbereiche und 20 Datumsbereiche. Mit **Markt verlassen** schließt der Bot seine Positionen, wenn ein Zeitraum beginnt, und handelt wieder, wenn er endet. Das Betreten und Verlassen eines Zeitraums steht im Protokoll und geht an die Kanäle des Bots, wenn er eine Position hält.

## Die Seite eines Bots {#a-robots-page}

Der Kopf hält den Status des Bots im Blick: seinen Namen, die Strategie und ihre Version, Badges für den Status, die Umgebung eines Broker-Bots (**Papier**, **Testnet**, **Live**), die Schutzregel (pausiert, gestoppt oder die Skalierung wie x0.5) und, auf einem Broker-Konto, **Scharf** oder **Entschärft**. Darunter: die nächste Auswertung als Countdown, die letzte Auswertung und die letzte Synchronisierung.

- **Jetzt ausführen** führt den Bot sofort aus. **Strategie stoppen** pausiert seinen Zeitplan; es schließt keine Positionen und räumt keine Orders ab. **Strategie starten** setzt ihn fort.
- **Insights** liest die geschlossenen Trades des Bots, siehe [Insights](/de/modules/journal#insights).
- Solange eine Schutzregel den Bot hält, sagt ein Hinweis warum, mit **Fortsetzen**.

Die Seite aktualisiert sich alle paar Sekunden; offene Positionen folgen dem Live-Preis, wo der Anbieter einen streamt. Acht Tabs:

| Tab | Zeigt |
|---|---|
| **Überblick** | **Kapital**, **Realisierter PnL**, **Unrealisierter PnL**, **Heute** (GuV und gesendete Orders), **Trefferquote**, **Profitfaktor**, **Max. Drawdown**, **Erwartungswert**; die Equity-Kurve; den letzten Preis jedes Instruments (live, gespeicherte Candle oder veraltet); **P&L je Instrument** und **P&L je Richtung**; den GuV-Kalender; einen Chart in der Zeiteinheit der Strategie mit Einstiegen und Ausstiegen. Ein Broker-Bot zeigt außerdem sein [Ausführungspanel](#on-a-broker-account) |
| **Positionen** | Instrument, Richtung, Menge in der Größe des Bots, Durchschnittspreis, Preis (live oder gespeichert), unrealisiert, Stop, Ziel und Alter |
| **Offen** | die Orders eines simulierten Bots, die auf die nächste Eröffnung warten; die Orders eines Broker-Bots, die beim Broker im Markt liegen |
| **Trades** | die geschlossenen Trades, 25 pro Seite, mit der Skalierung der Schutzregel, wo sie galt. **Nicht genommen** zählt die Einstiege, die der Bot abgelehnt hat, und listet die letzten 10, jeden mit seinem Grund |
| **Statistiken** | die Trade-Statistiken, die Performance-Kurve und bei Signalstrategien eine 3D-Streuung und die Serien |
| **Schutzregeln** | die Regeln als Sätze, was sie gerade halten, und der Verlauf der letzten 100 Auslösungen, Rücksetzungen und Fortsetzungen, neueste zuerst |
| **Protokoll** | die letzten 200 Ereignisse: Einstiege, Ausstiege, Ablehnungen, Aktionen der Schutzregeln, Fehler |
| **Einstellungen** | Strategie, Konto, Zeitplan, Priorität, Kapital und Schutzregeln, mit **Einstellungen des Bots**, der Aktualisierung auf die Version der Strategie und **Diesen Bot löschen** |

Das Buch eines Bots beginnt mit seinem ersten Lauf: Nur was ab der gerade geschlossenen Candle geschieht, wird erfasst. Was das Fenster davor zeigt, ist Historie, nie in seinem Buch, seinen Statistiken oder seinen Schutzregeln.

### Einstellungen {#settings}

**Einstellungen des Bots** bearbeitet den Namen, die Datenquelle jedes Instruments (**Standard des Anbieters** oder ein den Trading-Bots gewährter Konnektor), die Ausschlüsse, den Statusbericht, das Fenster, Priorität, Kapital, Schutzregeln und Benachrichtigungen. Strategie, Instrumente und Konto sind eingefroren: Eine andere Strategie oder ein anderes Konto ist ein neuer Bot. Der Hinweis **Vorwärts verfolgt** listet die Instrumente und die Ordertypen für Einstieg und Ausstieg der Strategie: Ihre Signale, ihr Stop und ihr Ziel laufen so, wie sie eingefroren wurden, während Kapital, Größe, Kalender und Kosten nicht gelesen werden.

**Auf die aktuelle Strategieversion umstellen** (angezeigt, wenn der Bot aus einer gespeicherten Strategie stammt) übernimmt die gespeicherte Strategie so, wie sie jetzt lautet. Das Buch des Bots wird geleert und beim nächsten Lauf neu aufgebaut, und ein scharf geschalteter Bot wird entschärft.

Das Löschen eines Bots löscht sein Buch, den Verlauf seiner Schutzregeln und sein Protokoll.

## Kapitalregel {#capital-policy}

Das Geld des Bots bemisst jede Entscheidung der Strategie: Die Strategie läuft mit dem Kapital des Bots, nicht mit ihrem eigenen. Jedes Feld ist optional:

| Feld | Bedeutung |
|---|---|
| **Zuteilung** | das Kapital, mit dem der Bot handelt. Leer: Kapital und Größe der Strategie |
| **Währung** | die Kontowährung, zum Lesen der Zahlen |
| **Anteil je Position (%)** | eine Position ist diesen Anteil des Kapitals des Bots wert. Braucht eine Zuteilung |
| **Hebel** | der Bot bemisst mit ihm, bis 125. Leer: 1 |
| **Einstiege pro Position** | wie viele Einstiege eine Position nehmen darf, bis 50. Leer: 1. Ein Broker-Bot nimmt nur 1 |
| **Max. offene Positionen** | ein Einstieg darüber hinaus wird nicht genommen |
| **Max. Orders pro Tag** | Einstiege pro UTC-Tag gezählt. Ausstiege werden nie zurückgehalten |

Jeder Einstieg wird dann mit der Skalierung einer Schutzregel multipliziert und auf die Größenstufe des Handelsplatzes abgerundet (nie aufgerundet). Auf einem Broker-Konto lehnt die beim Anlegen gesetzte **Maximale Position (Einheiten)** einen größeren Einstieg ab, statt ihn zu verkleinern.

**Ein abgelehnter Einstieg nennt seine Lösung**, im Tab Trades und im Protokoll:

> the bot already holds 3 position(s), its maximum of 3: the entry on BTCUSDT was not taken. Raise the maximum of positions

Die anderen lesen sich genauso: eine Größe unter der Stufe des Handelsplatzes (Zuteilung, Anteil je Position oder Hebel erhöhen) oder unter seinem Minimum (Zuteilung, Anteil je Position oder die Größe der Strategie erhöhen), das Tagesmaximum an Orders, eine Größe über der maximalen Position.

## Schutzregeln {#guards}

Schutzregeln sind Regeln über die **eigenen geschlossenen Trades** des Bots, in der Größe des Bots. Bis zu 10 je Bot. Ausstiege werden immer verwaltet, was auch immer eine Schutzregel hält.

Jede Schutzregel verbindet eine Bedingung mit einer Aktion, jede mit jeder.

| Wenn | Dann |
|---|---|
| **Verluste in Folge**, **Gewinne in Folge** (eine Anzahl) | **Pause** für Tage oder Stunden (eine Minute bis 365 Tage) |
| **Drawdown %**, **vom Höchststand** oder **vom Start** | **Halt bis zur Wiederaufnahme** |
| **Verlust % über**, **Verlustbetrag über**, **Verluste über**, **Gewinn % über** ein Fenster: **heute**, **diese Woche**, **diesen Monat**, **seit dem Start** (in der Zeitzone des Bots) | **Größe verringern**: **Teilen durch** mehr als 1 und höchstens 10, **Zurück zur vollen Größe** **nach Trades**, **nach Gewinnen**, **nach Tagen** oder **von Hand** |

Der Standardsatz für einen neuen Bot: einen Tag Pause nach 3 Verlusten in Folge, und die Größe halbieren nach 10 % Drawdown vom Höchststand, zurück zur vollen Größe nach 5 Trades.

- Ein pausierter oder gestoppter Bot nimmt keinen neuen Einstieg und verwaltet seine Positionen weiter. Ein verkleinerter Bot bemisst seine nächsten Einstiege kleiner.
- Eine Pause endet von selbst. Ein Halt wartet auf **Fortsetzen**. Eine Verkleinerung kehrt nach ihrer Rücksetzregel in einem Schritt zur vollen Größe zurück.
- Eine Zählregel zählt ab ihrer letzten Auslösung neu: Eine Pause nach drei Verlusten löst nach drei weiteren erneut aus. Eine Schwellenregel löst aus, wenn die Schwelle überschritten wird, und erneut, sobald sie nicht mehr erfüllt war oder ihr Fenster weitergerückt ist.
- Mehrere zugleich: Ein Halt schlägt eine Pause, eine Pause schlägt eine Verkleinerung, und eine mit ihnen ausgelöste Verkleinerung gilt, wenn die Einstiege wieder aufgenommen werden. Eine Verkleinerung auf eine Verkleinerung multipliziert sich, nie unter x0.1.
- Prozentangaben beziehen sich auf das Startkapital des Bots (die Zuteilung, sonst das Startkapital der Strategie). Ein Drawdown vom Höchststand bezieht sich auf das Höchstkapital; ein Verlust oder Gewinn über heute, diese Woche oder diesen Monat auf das Kapital zu Beginn dieses Zeitraums.
- **Fortsetzen** hebt alles auf, was eine Schutzregel hält. Das Bearbeiten der Regeln behält, was gerade gehalten wird.

Jede Auslösung und Rücksetzung steht im Tab **Schutzregeln** und im Protokoll und geht an die Kanäle des Bots.

## Priorität und Rate-Limits {#priority-and-rate-limits}

Jeder Bot hat eine **Priorität** von 0 bis 1000, standardmäßig 100. **Niedriger läuft zuerst**, wenn mehrere Bots gleichzeitig fällig sind oder sich einen Broker teilen. Bots, die beim selben Schluss fällig sind, laufen nebeneinander, bis zu 8 gleichzeitig.

Bots auf einem Broker-Konto werden alle 15 Sekunden synchronisiert, Broker für Broker, in Reihenfolge der Priorität:

- Ein Durchgang sendet höchstens 10 Orders an einen Broker, Bot für Bot in Reihenfolge der Priorität. Ausstiege und Schließungen werden von diesem Limit nie zurückgehalten; ein Einstieg über dem Limit wartet auf den nächsten Durchgang, und der Bot sagt das;
- wenn ein Broker Anfragen wegen seines Rate-Limits ablehnt, **warten nur die Bots auf diesem Broker**: bis zu der Zeit, die er genannt hat (höchstens eine Stunde), oder einen Durchgang lang, wenn er keine genannt hat. Jeder Bot zeigt einen Hinweis, der die Wartezeit nennt und was zu ändern ist (weniger Bots oder Instrumente auf dem Konto, oder ein höheres Limit in seinem Tarif). Bots auf anderen Brokern laufen weiter.

## Auf einem Broker-Konto {#on-a-broker-account}

Ein Bot kann seine Orders an jedes Konto senden, auf dem [Trading](/de/config/trading) eingeschaltet ist und das unter [Einstellungen → Broker](/de/config/brokers) den **Trading-Bots** gewährt ist, ein [Papierkonto](/de/config/paper-trading) eingeschlossen. Das Umgebungs-Badge sagt, wohin sie gehen: **Papier** (der Paper-Broker der App), **Testnet** (ein Test- oder Demokonto eines Handelsplatzes), **Live** (echtes Geld).

Die Engine entscheidet bei jedem Schluss, und das Konto führt aus. Beim Anlegen:

- Nur eine Signalstrategie führt auf einem Broker aus, mit einem Einstieg je Position (**Einstiege pro Position** 1);
- jedes Instrument wird beim Konto nachgeschlagen, das das gefundene Symbol und die Größenstufe anzeigt. Ein Instrument, das das Konto nicht benennen kann, wird abgelehnt: Der Bot rät nie;
- **Ausstiege**: **Beim Broker** (Stop und Ziel liegen beim Broker, an den Einstieg gehängt, wo der Broker sie halten kann) oder **Überwacht** (auf dem Live-Preis geprüft und beim Überschreiten zum Marktpreis geschlossen);
- mehrere Bots dürfen dasselbe Instrument auf demselben Konto nennen, aber nur einer ist jeweils scharf geschaltet, sodass zwei Bots nie eine Position gegeneinander handeln.

Ein Broker-Bot wird **entschärft angelegt**: Nichts wird gesendet, bis du ihn **scharf schaltest**, im Fenster **Name des Bots wird gestartet**, sobald sein erster Lauf fertig ist, oder später auf seiner Seite. Scharfschalten verlangt auf dem Konto unterzeichneten **Hintergrundhandel** (**Hintergrundhandel unterzeichnen** öffnet die Bedingungen zum Unterzeichnen) und deinen Authenticator-Code, wenn die [Trading-Sperre](/de/config/trading#trading-lock) an ist und das Konto kein Papierkonto ist. Ab dem nächsten Schluss gehen Einstiege zum Marktpreis raus und Ausstiege so, wie die Strategie entscheidet, ohne dass jemand zusieht. Jede Order passiert die [Risikolimits](/de/config/trading#risk-limits) des Kontos und steht im [Audit-Log](/de/config/trading#audit-log).

Das Scharfschalten liest zuerst das Konto: Es wird abgelehnt, wenn der Broker nicht erreichbar ist oder wenn das Konto weniger hält als das Geld des Bots (seine Zuteilung, sonst das Kapital der Strategie). Jeder Bot wird für sich geprüft, sodass sich mehrere Bots ein Konto teilen dürfen; ein Einstieg, den das Konto später nicht bezahlen kann, wird nicht gesendet, und das Protokoll sagt, was er brauchte und was das Konto hatte.

Das **Ausführungspanel** im **Überblick** zeigt die Stufe des Kontos, seine Limits, das Buch der Engine neben dem des Kontos und den simulierten Preis jedes Trades neben dem des Brokers. Sein Status:

| Status | Bedeutet |
|---|---|
| **Entschärft** | nichts wird gesendet, bis du den Bot scharf schaltest |
| **Abgeglichen** | das Konto hält, was der Bot erwartet |
| **Beeinträchtigt** | ein Einstieg wurde nur teilweise ausgeführt. Der Bot baut kein Exposure auf und verwaltet seine Ausstiege weiter, bis du glattstellst oder übernimmst |
| **Gestoppt, erneut scharf schalten** | auf dem Instrument des Bots wurde von Hand eine Position eröffnet, während der Bot flat war, oder die Position des Bots wurde von Hand geschlossen, aufgestockt oder verkleinert. Der Bot wurde entschärft und seine wartenden Einstiege storniert; bei einer geschlossenen Position werden auch ihre Ausstiege storniert. Schalte ihn erneut scharf, um weiterzumachen |
| **Senden pausiert** | eine Schutzregel hält neue Einstiege, oder der Handel ist auf dem Konto angehalten: Nur was Risiko abbaut, wird gesendet |
| **Beim Broker prüfen** | eine Order hat keine Antwort bekommen: Auf diesem Instrument wird nichts mehr gesendet, bis das geklärt ist |
| **Broker nicht erreichbar** | der Broker hat drei Synchronisierungen in Folge (45 s) nicht geantwortet. Nichts wird gesendet, bis er antwortet, und das Konto wird erneut gelesen, bevor etwas gesendet wird |
| **Kontofehler** | das Konto kann nicht genutzt werden (seine Zugangsdaten oder seine Freigabe): Nichts wird gesendet, bis es wieder geht |

Der Bot liest das Konto bei jeder Synchronisierung (alle 15 s und als Erstes nach einem Neustart), bevor er etwas sendet. Jede Änderung, die du von Hand an der Position des Bots machst, stoppt ihn, bis du ihn erneut scharf schaltest. Einen Stop oder ein Ziel, die du beim Broker von Hand verschiebst, bleiben, wo du sie hingelegt hast: Der Bot verschiebt sie nicht mehr. Storniere Stop und Ziel beide von Hand, setzt die nächste Synchronisierung sie einmal neu; danach überwacht der Bot sie auf dem Live-Preis. Das Stornieren einer anderen seiner ausstehenden Orders ändert für ihn nichts. Die Positionsprüfung des Kontos überlässt das Instrument eines Bots dem Bot: Die Trades eines Bots halten das Konto nie an.

**Positionen** und **Offen** zeigen nur, was beim Broker liegt. Ein Einstieg, der nicht rausging (abgelehnt von einem Limit, einer Schutzregel, den Größenregeln des Handelsplatzes, dem Guthaben des Kontos oder dem Broker), ist eine Zeile im **Protokoll**, sein genauer Grund beim Darüberfahren. Preise und GuV eines geschlossenen Trades sind das, wozu seine Orders beim Broker ausgeführt wurden, Gebühren eingeschlossen, sobald sein Ausstieg ausgeführt ist; bis dahin gilt die simulierte Zahl. Ein Stop oder ein Ziel, das beim Broker ausgeführt wird, **Stoppen und glattstellen** oder ein Schließen, das du beim Broker machst, schließt den Trade des Bots bei dieser Ausführung, auch wenn die eigenen Candles des Bots das Niveau nie erreicht haben; ein Einstieg, der nie rausging, wird nicht genommen. Eine Candle, die der Anbieter nach ihrem Schluss korrigiert, ändert nichts: Der Bot tut, was er bei diesem Schluss entschieden hat, und eine Umkehr ist eine Nachricht.

- **Entschärfen** stoppt das Senden. Orders, die bereits beim Broker liegen, bleiben dort.
- **Stoppen und glattstellen** entschärft, storniert die aktiven Orders des Bots und schließt zum Marktpreis, was er hält.
- **Die Differenz übernehmen** überlässt dir, was das Konto außerhalb des Bots hält, sodass ein beeinträchtigter Bot wieder handelt.

Scharfschalten, Entschärfen, Glattstellen und Übernehmen tust immer du auf der Seite, nie ein Agent oder ein Workflow.

## Paper-Sitzungen {#paper-sessions}

Die Paper-Sitzungen des Backtests sind jetzt Bots, mit ihrem Buch, ihren Ereignissen, ihrem Konto und ihren Kanal-Freigaben. Ein alter Link auf eine Paper-Sitzung öffnet die Seite des Bots. **Als Bot starten** ersetzt **Paper Trade** im Backtest.

## Gut zu wissen {#good-to-know}

- [KI-Agenten (MCP)](/de/config/ai-agents) können Bots lesen (die Liste, einen einzelnen Bot, sein Dashboard, seine Trades, seine Schutzregel-Ereignisse, sein Ausführungspanel, die Flotte, den Posteingang und die Nachrichtenvariablen). Einen anlegen, bearbeiten, ausführen oder scharf schalten bleibt in der App.
- Ein Bot liest die Candles und den Live-Preis jedes Instruments über die dafür gewählte Datenquelle, sonst über einen den Trading-Bots gewährten [Daten-Konnektor](/de/config/connectors), sonst über einen dem Backtest gewährten, sonst über den ersten Konnektor des Anbieters.
- Im [Demo-Modus](/de/guide/demo) ist das Modul nur lesend: zwei vorbelegte Bots auf einem Demo-Papierkonto, einer von seiner Schutzregel verkleinert und einer pausiert, keiner scharf geschaltet oder laufend.
- Wie ein simulierter Bot auf dem Live-Preis ausführt, und die Optionen, die er mit dem Backtest teilt: [Backtest](/de/modules/market-data#backtest).
