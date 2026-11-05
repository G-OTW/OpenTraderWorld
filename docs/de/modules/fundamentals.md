# Fundamentals

Ein Ort, um die Wirtschaft und ein Unternehmen aus den Quellen zu lesen, die die Zahlen veröffentlichen: Makroreihen mit Charts, Unternehmensabschlüsse, SEC-Filings, Earnings-Call-Transkripte, ETF-Bestände, ein Marktkalender und Alternativdaten. Alles wird in deiner eigenen Datenbank gespeichert, sodass Charts, der [Agent](/de/modules/agent) und andere Module es lesen, ohne den Anbieter erneut zu fragen.

Fundamentals hat keine eigenen Anbieter-Einstellungen. Jede Quelle ist ein **[Daten-Konnektor](/de/config/connectors)**, der dem Modul gewährt wird: Das Stecker-Symbol in der Seitenkopfzeile öffnet die gemeinsame Konnektor-Ansicht. Viele Quellen sind **schlüssellose** öffentliche Stellen (SEC EDGAR, das US Treasury, die EZB, Eurostat, die BIZ, die OECD, der IWF, die Weltbank, die CFTC, FINRA, USAspending); der Rest nimmt einen kostenlosen oder bezahlten Schlüssel, den du mitbringst. Nichts wird geholt, bis du einen Konnektor hinzufügst und ihn Fundamentals gewährst.

## Seiten

| Seite | Was sie zeigt |
|---|---|
| **Macro** | Deine Reihen nach Kategorie (Wachstum, Inflation, Arbeitsmarkt, Zinsen, Geld, Umfragen, Immobilien, Energie, Fiskal, Positionierung), bis zu vier in einem Chart, die Zinsstrukturkurve des Treasury und Leitzinsen der Zentralbanken. |
| **Company** | Profil und Kennzahlen, Abschlüsse, Schätzungen, Earnings, Segmente, Dividenden und Rückkäufe, Eigentümerstruktur, Peers, ESG und Vergütung, Filings und Transkripte. |
| **ETF** | Profil, Top-Bestände, Sektor- und Länderengagement. |
| **Events** | Anstehende Earnings, IPOs, Corporate Actions und Zentralbankentscheide. |
| **Documents** | Jedes gespeicherte Filing und Transkript, mit Volltextsuche und dem Transkript-Reader. |
| **Alternative data** | Kongress-Trades, Lobbyausgaben, Bundesaufträge und erteilte Patente. |
| **Library** | Tabs für die Reihen und Unternehmen, die du behältst (filterbar), die Quellenpriorität und welcher Anbieter welche Datenfamilie bedient. |

**Anpassen** (oben rechts) legt Dichte, sichtbare Abschnitte und deren Reihenfolge fest, Seite für Seite.

## Makroreihen {#macro}

**Reihe hinzufügen** durchsucht den Katalog eines Anbieters oder nimmt den eigenen Code des Anbieters (`CPIAUCSL` bei FRED, `HICP/M.U2.N.000000.4D0.ANR` bei der EZB). Ein Code wird beim Anbieter geprüft, bevor etwas gespeichert wird: Ein unbekannter Code ist ein Fehler, der ihn nennt, nie eine leere Reihe. **Starter-Set hinzufügen** fügt mit einem Klick eine erste Auswahl US- und Euroraum-Reihen hinzu.

| Anbieter | Schlüssel | Was er abdeckt |
|---|---|---|
| FRED | kostenlos | Die meisten US-Reihen (spiegelt auch BLS, BEA und Census) |
| US Treasury | keiner | Tägliche Par-Zinsstrukturkurve, gesamte Staatsverschuldung |
| EZB, Eurostat | keiner | Euroraum-Inflation, Zinsen, Geld, BIP, Arbeitslosigkeit |
| BIZ | keiner | Leitzinsen der Zentralbanken, effektive Wechselkurse |
| OECD, IWF, Weltbank | keiner | Frühindikatoren, World Economic Outlook, jährliche Länderdaten |
| BLS, BEA, EIA, US Census | kostenlos | US-Details, wenn FRED hinterherhinkt oder eine Reihe fehlt |
| CFTC | keiner | Commitments of Traders, nicht-kommerzielle Netto-Positionierung |

Eine Zeile ist eine **Periode**: Eine Beobachtung wird zum Beginn der Periode gespeichert, die sie abdeckt. Der Chart berechnet die Transformationen beim Lesen (Niveau, Veränderung zum Vorjahr, Veränderung zur Vorperiode, Differenz, Index 100), es wird also nie etwas Abgeleitetes gespeichert. Die Veränderung zum Vorjahr vergleicht jeden Wert mit dem, der ein Jahr früher datiert ist; eine fehlende Periode zeigt eine Lücke statt eines Vergleichs mit dem falschen Monat. Die Rezessionsschattierung folgt den NBER-Daten.

## Unternehmen {#company}

Company, ETF und Alternative data teilen sich eine **Symbolauswahl**: zuerst deine Favoriten, dann die 15 zuletzt geöffneten. Ihre Suche deckt jedes gespeicherte Symbol ab, plus EDGAR-Treffer für Unternehmen.

Tippe einen Ticker. Er wird in der Ticker-Liste von SEC EDGAR aufgelöst; ein Ticker, den EDGAR nicht kennt, ist ein Fehler, nie ein bester Treffer. Das Öffnen eines Unternehmens speichert es und holt im Hintergrund:

- **Abschlüsse** aus den XBRL-Company-Facts, jährlich und quartalsweise. Vierte Quartale und Year-to-date-Cashflow-Zeilen werden per Differenz abgeleitet; jede Zeile behält das Tag, unter dem sie gemeldet wurde.
- **Filings** (10-K, 10-Q, 8-K, Proxies...) mit einem Link zur Quelle.
- **Insider-Trades**, aus Form 4 geparst.

Die anderen Tabs lesen die Aggregatoren, die du verbindest, beste Quelle zuerst. Ein Anbieter, dessen Tarif einen Datensatz auslässt (ein kostenloser FMP-Schlüssel und die Earnings-Historie, zum Beispiel), übergibt an den nächsten, und ein dem Modul gewährter Konnektor ohne seinen Schlüssel wird übersprungen. Beim Preis gewinnt die längste Historie (kostenlose Tarife enden oft bei einem oder zwei Jahren):

| Daten | Anbieter |
|---|---|
| Schätzungen, Kursziele, Rating-Aktionen | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Earnings (EPS-Schätzung und -Ist, nächster Termin) | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Segmente | Financial Modeling Prep |
| Dividenden und Splits | EODHD, Massive, Financial Modeling Prep, Alpha Vantage |
| 13F-Halter | Financial Modeling Prep |
| Short Interest | FINRA, Massive |
| Peers | Financial Modeling Prep, Finnhub |
| ESG und Managervergütung | Financial Modeling Prep, Finnhub |
| Transkripte | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Preis und Marktkennzahlen | jeder Marktdaten-Konnektor mit täglichen Aktien-Bars |

**Library → Quellenpriorität** listet jeden Datensatz mit mehr als einer Quelle (Transkripte eingeschlossen) in der Reihenfolge, in der seine Anbieter probiert werden. Wähle neben einem Anbieter einen Rang, um ihn dorthin zu verschieben; **Standardreihenfolge** stellt die Reihenfolge der App wieder her. Der Preis hat keine Reihenfolge: Die längste Historie gewinnt.

Jeder Tab sagt, welcher Anbieter wann geantwortet hat, oder nennt den Fehler mit der Lösung (meist ein hinzuzufügender Konnektor oder ein Tarif, der den Datensatz nicht enthält). Eine Antwort wird aufbewahrt und wiederverwendet, bis sie veraltet (einige Stunden beim Kalender, ein Tag bei Schätzungen, eine Woche bei Haltern); **Aktualisieren** fragt jetzt erneut.

Das Öffnen einer Seite verbraucht dein Kontingent nicht bei einem Anbieter, der gerade abgelehnt hat: Einer, der den Datensatz abgewiesen hat (Tarif, Symbol, Rate-Limit), wird eine Weile in Ruhe gelassen, von wenigen Minuten nach einem Netzwerkfehler bis zu einer Woche nach einer Tarifablehnung, ebenso einer, dessen auf seinem Konnektor angegebenes Kontingent aufgebraucht ist. Der Tab sagt das und wann der nächste automatische Versuch ist; **Aktualisieren** fragt jeden Anbieter auf einmal.

**Folge** einem Unternehmen, damit es täglich aktualisiert wird und du über neue Filings informiert wirst.

### Transkripte {#transcripts}

Der Tab Transkripte listet die Calls, die ein Anbieter zum Unternehmen hält; der Text eines Transkripts wird beim ersten Öffnen geholt, in Sprecherbeiträge zerlegt, vorbereitete Statements getrennt von den Fragen und Antworten, und ist zusammen mit den anderen Dokumenten durchsuchbar.

## Alternative Daten {#alt}

Das Unternehmen ist das in Company geöffnete; ein hier getippter Ticker wird zuerst geöffnet.

| Tab | Anbieter |
|---|---|
| Kongress-Trades | Quiver Quant (die neuesten über alle Mitglieder oder die eines Unternehmens), Finnhub Premium (pro Unternehmen) |
| Lobbying | LDA.gov, Quiver Quant |
| Staatsaufträge | USAspending, Quiver Quant |
| Patente | USPTO Open Data Portal (kostenloser Schlüssel), Quiver Quant |

Die öffentlichen Quellen kennen ein Unternehmen unter seinem **eingetragenen Namen**, nicht unter seinem Ticker. LDA.gov, USAspending und das USPTO werden mit dem Namen abgefragt, den EDGAR speichert, und ein Datensatz zählt nur, wenn sein Name nach Weglassen von Satzzeichen und Rechtsformzusatz derselbe ist (`Lockheed Martin Corp` passt zu `LOCKHEED MARTIN CORPORATION`, nie zu `Lockheed Martin Aculight`). Jeder Tab zeigt den Namen, auf den er passte. Bei Aufträgen wird der übergeordnete Empfänger verwendet, sodass darunter eingetragene Tochtergesellschaften zählen und eine separat registrierte (Amazon Web Services unter Amazon) nicht.

LDA.gov nimmt einen kostenlosen Schlüssel (Registrierung auf lda.gov). Seine Firewall weist Netzwerke außerhalb der USA ab (ein HTTP 403, der sie nennt): Erreiche es von einer US-Verbindung oder nutze Quiver Quant. Ein Report zählt einmal: Eine Änderung ersetzt das Original, und Lobbyisten-Registrierungen, die keine Ausgaben tragen, bleiben außen vor. Ein Unternehmen, das über eine Tochter unter anderem Namen lobbyiert (JPMorgan Chase Holdings für JPMorgan Chase), zeigt nur die Reports unter dem eigenen Namen.

## Anbieterabdeckung {#coverage}

Welcher Anbieter welche Daten bedienen kann, wie unter **Library → Anbieterabdeckung**. Eine von mehreren Anbietern bediente Familie wird in der Reihenfolge von **Library → Quellenpriorität** probiert. Der Preis und die Marktkennzahlen kommen von jedem Marktdaten-Konnektor mit täglichen Aktien-Bars (Alpha Vantage, EODHD, Massive, Yahoo, IBKR...), hier nicht aufgeführt.

| Anbieter | Schlüssel | Makro | COT | Abschlüsse | Filings | Insider | Schätzungen | Earnings | Segmente | Dividenden | Halter | Short Interest | Peers | ESG | ETF | Kalender | Transkripte | Alt-Daten |
|---|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| SEC EDGAR | keiner |  |  | ✓ | ✓ | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |
| FRED (St. Louis Fed) | kostenlos | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Treasury | keiner | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| EZB Data Portal | keiner | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| Eurostat | keiner | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BIZ | keiner | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| OECD | keiner | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| IWF | keiner | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| Weltbank | keiner | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BLS | kostenlos | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BEA | kostenlos | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| EIA | kostenlos | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Census | kostenlos | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| CFTC | keiner |  | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| FINRA | keiner |  |  |  |  |  |  |  |  |  |  | ✓ |  |  |  |  |  |  |
| Financial Modeling Prep | Gratis-Tarif |  |  |  |  |  | ✓ | ✓ | ✓ | ✓ | ✓ |  | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| Finnhub | Gratis-Tarif |  |  |  |  |  | ✓ | ✓ |  |  |  |  | ✓ | ✓ |  | ✓ | ✓ | ✓¹ |
| Alpha Vantage | Gratis-Tarif |  |  |  |  |  | ✓ | ✓ |  | ✓ |  |  |  |  | ✓ | ✓ | ✓ |  |
| EODHD | Gratis-Tarif |  |  |  |  |  |  |  |  | ✓ |  |  |  |  | ✓ | ✓ |  |  |
| Massive (Polygon.io) | Gratis-Tarif |  |  |  |  |  |  |  |  | ✓ |  | ✓ |  |  |  |  |  |  |
| USAspending | keiner |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| LDA.gov (Lobbying) | kostenlos |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| USPTO Open Data Portal | kostenlos |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| Quiver Quant | bezahlt |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |

¹ Finnhub bedient Kongress-Trades nur in einem Premium-Tarif.

Richtwerte: Tarife ändern sich, und ein Gratis-Tarif kann eine Familie auslassen (Financial Modeling Prep gratis: keine 13F-Halter, Transkripte oder ETF-Bestände; Finnhub gratis: kein ESG und keine Kongress-Trades; Alpha Vantage gratis: 25 Anfragen pro Tag). Prüfe die Seite des Anbieters, bevor du zahlst.

## Automatische Aktualisierung und Benachrichtigungen {#refresh}

Gespeicherte Reihen werden aktualisiert, sobald ihre Frequenz sagt, dass ein neuer Wert da sein kann: eine tägliche Reihe zweimal am Tag, eine wöchentliche oder monatliche täglich, eine vierteljährliche alle drei Tage, eine jährliche wöchentlich. Verfolgte Unternehmen werden einmal am Tag von EDGAR aktualisiert. Eine Quelle ohne gewährten Konnektor wird übersprungen.

Eine Reihe mit neuer Periode und ein verfolgtes Unternehmen mit neuem Filing (Insider-Formulare ausgenommen) lösen eine Benachrichtigung aus, die an die [Kanäle](/de/config/settings#notifications) gepusht wird, die Fundamentals gewährt wurden (die Glocke in der Seitenkopfzeile).

## Dashboard-Widgets {#dashboard}

Das [Dashboard](/de/modules/dashboard) bietet Makroreihen und -Boards, Unternehmens-Snapshots, Abschlusshistorie, Unternehmen, Filings, anstehende Earnings und Bewertungsvergleiche. Karten zeigen Währung, Berichtsbasis und Daten; ihre Aktualisierung liest nur gespeicherte Daten. Anstehende Earnings nutzt den gespeicherten Earnings-Snapshot eines Unternehmens oder, wenn er kein künftiges Datum hat, den gespeicherten Marktkalender. Die Bewertung nutzt manuell gewählte gespeicherte Unternehmen oder die gespeicherten Peers aus deren Tab Peers. Fehlende Schätzungen und Kennzahlen bleiben nicht verfügbar, statt abgeleitet zu werden.

## Suche und Agenten {#search}

Die Suche in der oberen Leiste findet bei aktivierten Inhaltstiteln gespeicherte Unternehmen, Reihen und Dokumenttitel.

Agenten erreichen das Gespeicherte schreibgeschützt über das [MCP-Gateway](/de/config/ai-agents), sobald einem Token **Fundamentals** gewährt ist: Reihen und Beobachtungen, Unternehmen, Abschlüsse, Filings, Transkripte, Insider-Trades und jeden gespeicherten Datensatz. Einen Anbieter abzufragen, zu aktualisieren und Reihen hinzuzufügen bleibt außen vor: Das verbraucht dein Anbieterkontingent.
