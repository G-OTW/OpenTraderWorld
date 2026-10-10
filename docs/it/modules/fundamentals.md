# Fundamentals

Un posto solo per leggere l'economia e un'azienda dalle fonti che pubblicano i numeri: serie macro con grafici, bilanci aziendali, filing SEC, trascrizioni delle earnings call, holding degli ETF, un calendario di mercato e dati alternativi. Tutto è salvato nel tuo database, così grafici, l'[Agent](/it/modules/agent) e altri moduli lo leggono senza interrogare di nuovo il provider.

Fundamentals non ha impostazioni di provider proprie. Ogni fonte è un **[data connector](/it/config/connectors)** concesso al modulo: l'icona della spina nell'intestazione della pagina apre la schermata condivisa dei connector. Molte fonti sono enti pubblici **senza chiave** (SEC EDGAR, il Tesoro USA, la BCE, Eurostat, la BIS, l'OCSE, l'IMF, la Banca Mondiale, la CFTC, FINRA, USAspending); le altre richiedono una chiave gratuita o a pagamento che porti tu. Nulla viene scaricato finché non aggiungi un connector e lo concedi a Fundamentals.

## Pagine

| Pagina | Cosa mostra |
|---|---|
| **Macro** | Le tue serie per categoria (crescita, inflazione, lavoro, tassi, moneta, sondaggi, immobiliare, energia, fiscale, posizionamento), fino a quattro su un grafico, la curva dei rendimenti del Tesoro e i tassi di policy delle banche centrali. |
| **Company** | Profilo e metriche chiave, bilanci, stime, utili, segmenti, dividendi e buyback, proprietà, peer, ESG e retribuzioni, filing e trascrizioni. |
| **ETF** | Profilo, principali holding, esposizione per settore e paese. |
| **Events** | Prossimi utili, IPO, operazioni societarie e decisioni delle banche centrali. |
| **Documents** | Ogni filing e trascrizione salvati, con ricerca full-text e lettore di trascrizioni. |
| **Alternative data** | Operazioni del Congresso, spesa in lobbying, appalti federali e brevetti concessi. |
| **Library** | Schede per le serie e le aziende che conservi (filtrabili), la priorità delle fonti e quale provider serve quale famiglia di dati. |

**Personalizza** (in alto a destra) imposta la densità, quali sezioni mostrare e in che ordine, pagina per pagina.

## Serie macro {#macro}

**Aggiungi serie** cerca nel catalogo di un provider o prende il codice del provider stesso (`CPIAUCSL` su FRED, `HICP/M.U2.N.000000.4D0.ANR` sulla BCE). Un codice viene verificato presso il provider prima che venga salvato qualcosa: un codice sconosciuto è un errore che lo nomina, mai una serie vuota. **Aggiungi un set iniziale** aggiunge con un clic una prima selezione di serie USA e dell'area euro.

| Provider | Chiave | Cosa copre |
|---|---|---|
| FRED | gratuita | La maggior parte delle serie USA (rispecchia anche BLS, BEA e Census) |
| US Treasury | nessuna | Curva dei rendimenti par giornaliera, debito pubblico totale |
| ECB, Eurostat | nessuna | Inflazione, tassi, moneta, PIL, disoccupazione dell'area euro |
| BIS | nessuna | Tassi di policy delle banche centrali, tassi di cambio effettivi |
| OECD, IMF, World Bank | nessuna | Indicatori anticipatori, World Economic Outlook, dati annuali per paese |
| BLS, BEA, EIA, US Census | gratuita | Dettaglio USA quando FRED è in ritardo o manca di una serie |
| CFTC | nessuna | Commitments of Traders, posizionamento netto non commerciale |

Una riga è un **periodo**: un'osservazione è salvata rispetto all'inizio del periodo che copre. Il grafico calcola le trasformazioni in lettura (livello, anno su anno, variazione del periodo, differenza, indice 100), quindi nulla di derivato viene mai salvato. L'anno su anno confronta ogni valore con quello datato un anno prima; un periodo mancante mostra un buco invece di un confronto con il mese sbagliato. L'ombreggiatura delle recessioni segue le date NBER.

## Aziende {#company}

Company, ETF e Alternative data condividono un unico **selettore di simboli**: prima i tuoi preferiti, poi i 15 aperti più di recente. La sua ricerca copre ogni simbolo salvato, più le corrispondenze EDGAR per le aziende.

Digita un ticker. Viene risolto sull'elenco dei ticker di SEC EDGAR; un ticker che EDGAR non conosce è un errore, mai un'ipotesi. Aprire un'azienda la salva e preleva, in background:

- **Bilanci** dai company facts XBRL, annuali e trimestrali. I quarti trimestri e le voci di flusso di cassa da inizio anno sono derivati per differenza; ogni voce conserva il tag con cui è stata riportata.
- **Filing** (10-K, 10-Q, 8-K, proxy...) con un link alla fonte.
- **Operazioni degli insider** analizzate dal Form 4.

Le altre schede leggono gli aggregatori che colleghi, prima la fonte migliore. Un provider il cui piano lascia fuori un dataset (una chiave FMP gratuita e lo storico degli utili, per dire) passa la mano al successivo, e un connector concesso al modulo senza la sua chiave viene saltato. Per il prezzo vince lo storico più lungo (i piani gratuiti spesso si fermano a uno o due anni):

| Dati | Provider |
|---|---|
| Stime, target di prezzo, azioni di rating | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Utili (EPS stimato ed effettivo, prossima data) | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Segmenti | Financial Modeling Prep |
| Dividendi e split | EODHD, Massive, Financial Modeling Prep, Alpha Vantage |
| Detentori 13F | Financial Modeling Prep |
| Short interest | FINRA, Massive |
| Peer | Financial Modeling Prep, Finnhub |
| ESG e retribuzioni dei dirigenti | Financial Modeling Prep, Finnhub |
| Trascrizioni | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Prezzo e rapporti di mercato | qualsiasi connector di dati di mercato con barre giornaliere di azioni |

**Library → Priorità delle fonti** elenca ogni dataset con più di una fonte (trascrizioni comprese) nell'ordine in cui i suoi provider vengono provati. Scegli una posizione accanto a un provider per spostarlo lì; **Ordine predefinito** ripristina l'ordine dell'app. Il prezzo non ha un ordine: vince lo storico più lungo.

Ogni scheda dice quale provider ha risposto e quando, oppure l'errore che indica la soluzione (di solito un connector da aggiungere o un piano che non include il dataset). Una risposta viene conservata e riutilizzata finché non diventa obsoleta (qualche ora per il calendario, un giorno per le stime, una settimana per i detentori); **Aggiorna** chiede di nuovo adesso.

Aprire una pagina non consuma la tua quota presso un provider che ha appena rifiutato: uno che ha respinto il dataset (piano, simbolo, rate limit) viene lasciato stare per un po', da pochi minuti dopo un errore di rete a una settimana dopo un rifiuto di piano, e lo stesso vale per uno la cui quota, dichiarata sul suo connector, è esaurita. La scheda lo dice e indica quando è il prossimo tentativo automatico; **Aggiorna** interroga tutti i provider insieme.

**Segui** un'azienda per farla aggiornare ogni giorno e per essere avvisato dei suoi nuovi filing.

### Trascrizioni {#transcripts}

La scheda Trascrizioni elenca le call che un provider ha per l'azienda; il testo di una trascrizione viene prelevato la prima volta che la apri, diviso in interventi per speaker, con le dichiarazioni preparate separate dal Q&A, e ricercabile insieme agli altri documenti.

## Dati alternativi {#alt}

L'azienda è quella aperta in Company; un ticker digitato qui viene aperto prima.

| Scheda | Provider |
|---|---|
| Operazioni del Congresso | Quiver Quant (le più recenti di tutti i membri, o quelle di una azienda), Finnhub premium (per azienda) |
| Lobbying | LDA.gov, Quiver Quant |
| Appalti governativi | USAspending, Quiver Quant |
| Brevetti | USPTO Open Data Portal (chiave gratuita), Quiver Quant |

Le fonti pubbliche conoscono un'azienda dal suo **nome registrato**, non dal ticker. LDA.gov, USAspending e l'USPTO vengono interrogati con il nome che EDGAR salva, e un record conta solo quando il suo nome è lo stesso una volta tolti punteggiatura e suffisso legale (`Lockheed Martin Corp` corrisponde a `LOCKHEED MARTIN CORPORATION`, mai a `Lockheed Martin Aculight`). Ogni scheda mostra il nome con cui ha trovato la corrispondenza. Per gli appalti si usa il beneficiario capogruppo, quindi le controllate archiviate sotto di esso contano e una registrata separatamente (Amazon Web Services sotto Amazon) no.

LDA.gov richiede una chiave gratuita (registrati su lda.gov). Il suo firewall respinge le reti fuori dagli USA (un HTTP 403 che lo nomina): raggiungilo da una connessione USA, oppure usa Quiver Quant. Un report conta una volta: un emendamento sostituisce l'originale, e le registrazioni dei lobbisti, che non portano spesa, sono lasciate fuori. Un'azienda che fa lobbying tramite una controllata con un altro nome (JPMorgan Chase Holdings per JPMorgan Chase) mostra solo i report archiviati con il suo nome.

## Copertura dei provider {#coverage}

Quale provider può servire quali dati, come in **Library → Copertura dei provider**. Una famiglia servita da più provider viene provata nell'ordine di **Library → Priorità delle fonti**. Il prezzo e i rapporti di mercato vengono da qualsiasi connector di dati di mercato con barre giornaliere di azioni (Alpha Vantage, EODHD, Massive, Yahoo, IBKR...), non elencati qui.

| Provider | Chiave | Macro | COT | Statements | Filings | Insiders | Estimates | Earnings | Segments | Dividends | Holders | Short interest | Peers | ESG | ETF | Calendar | Transcripts | Alt data |
|---|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| SEC EDGAR | nessuna |  |  | ✓ | ✓ | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |
| FRED (St. Louis Fed) | gratuita | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Treasury | nessuna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| ECB Data Portal | nessuna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| Eurostat | nessuna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BIS | nessuna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| OECD | nessuna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| IMF | nessuna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| World Bank | nessuna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BLS | gratuita | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BEA | gratuita | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| EIA | gratuita | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Census | gratuita | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| CFTC | nessuna |  | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| FINRA | nessuna |  |  |  |  |  |  |  |  |  |  | ✓ |  |  |  |  |  |  |
| Financial Modeling Prep | piano gratuito |  |  |  |  |  | ✓ | ✓ | ✓ | ✓ | ✓ |  | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| Finnhub | piano gratuito |  |  |  |  |  | ✓ | ✓ |  |  |  |  | ✓ | ✓ |  | ✓ | ✓ | ✓¹ |
| Alpha Vantage | piano gratuito |  |  |  |  |  | ✓ | ✓ |  | ✓ |  |  |  |  | ✓ | ✓ | ✓ |  |
| EODHD | piano gratuito |  |  |  |  |  |  |  |  | ✓ |  |  |  |  | ✓ | ✓ |  |  |
| Massive (Polygon.io) | piano gratuito |  |  |  |  |  |  |  |  | ✓ |  | ✓ |  |  |  |  |  |  |
| USAspending | nessuna |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| LDA.gov (lobbying) | gratuita |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| USPTO Open Data Portal | gratuita |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| Quiver Quant | a pagamento |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |

¹ Finnhub serve le operazioni del Congresso solo con un piano premium.

Indicativo: i piani cambiano, e un piano gratuito può lasciare fuori una famiglia (Financial Modeling Prep gratuito: niente detentori 13F, trascrizioni o holding ETF; Finnhub gratuito: niente ESG o operazioni del Congresso; Alpha Vantage gratuito: 25 richieste al giorno). Controlla la pagina del provider prima di pagare.

## Aggiornamento automatico e notifiche {#refresh}

Le serie salvate vengono aggiornate quando la loro frequenza dice che può essere uscito un nuovo valore: una serie giornaliera due volte al giorno, una settimanale o mensile ogni giorno, una trimestrale ogni tre giorni, una annuale ogni settimana. Le aziende seguite vengono aggiornate da EDGAR una volta al giorno. Una fonte senza connector concesso viene saltata.

Una serie con un nuovo periodo e un'azienda seguita con un nuovo filing (moduli insider esclusi) generano una notifica, inviata ai [canali](/it/config/settings#notifications) concessi a Fundamentals (la campanella nell'intestazione della pagina).

## Widget della dashboard {#dashboard}

La [Dashboard](/it/modules/dashboard) offre serie e bacheche macro, snapshot aziendali, storico dei bilanci, aziende, filing, prossimi utili e confronti di valutazione. Le schede mostrano valuta, base di rendicontazione e date; il loro aggiornamento legge solo dati salvati. Prossimi utili usa lo snapshot degli utili salvato di un'azienda o, quando non ha una data futura, il calendario di mercato salvato. La valutazione usa aziende salvate selezionate a mano o i peer salvati dalla sua scheda Peers. Stime e rapporti mancanti restano non disponibili invece di essere dedotti.

## Ricerca e agent {#search}

La ricerca nella barra in alto, con i titoli dei contenuti attivi, trova aziende salvate, serie e titoli di documenti.

Gli agent raggiungono ciò che è salvato, in sola lettura, tramite il [gateway MCP](/it/config/ai-agents) una volta che a un token è concesso **Fundamentals**: serie e osservazioni, aziende, bilanci, filing, trascrizioni, operazioni degli insider e ogni dataset salvato. Cercare presso un provider, aggiornare e aggiungere serie restano fuori: consumano la quota del tuo provider.
