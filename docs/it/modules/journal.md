# Trading Journal

Registra ogni operazione, in qualsiasi valuta, e ottieni statistiche di performance oneste: curva di equity, win rate, expectancy, profit factor, drawdown, Sharpe e altro. Il journal è organizzato in dieci schede: **Breakdown**, **Analytics**, **PnL Calendar**, **Trades**, **Strategies & capital**, **Tags**, **Templates**, **Fees & currency**, **Import**, **Pending tasks**.

## Categorie

Le operazioni vivono in **categorie**: cartelle come *Crypto scalping* o *Long-term stocks*, ciascuna con il proprio colore, capitale e statistiche. Creale dalla barra delle categorie; trascina per riordinare. Eliminare una categoria elimina le sue operazioni.

## Imposta il capitale

In **Strategies & capital**, dai a ogni categoria uno **stack iniziale** e registra **rabbocchi** e **prelievi** nel tempo. È rispetto a questo che vengono calcolati rendimento, curva di equity e drawdown; senza, ottieni comunque il PnL, ma non i rendimenti.

Puoi anche dare un nome alle **strategie** con i nomi dei loro segnali (ad es. *Breakout, Pullback*). Tagga le operazioni con una strategia/segnale e la scheda Breakdown può filtrare per essi: è così che scopri quali setup pagano davvero.

## Template

I template guidano il modulo dell'operazione. Esiste una **operazione standard** predefinita; creane di tuoi per mercato o stile:

- I **campi riservati** (side, prezzi, quantità, fee, leva, moltiplicatore, valuta, tipo di unità…) alimentano le statistiche di performance.
- I **campi personalizzati** (testo, numeri, elenchi di scelta…) sono liberi: voto del setup, condizione di mercato, qualsiasi cosa tu tracci.
- Un template può impostare una **struttura di fee predefinita**, preselezionata quando registri da esso (modificabile per operazione).

## Registrare le operazioni

Dalla scheda Trades, scegli un template (o quello *Quick*, che mostra tutti i campi) e compila il modulo. Due livelli:

- **Simple**: un ingresso, un'uscita (o lascia vuota l'uscita per una posizione aperta).
- **Advanced**: scaling in/out con più **gambe di ingresso e uscita** (ciascuna con il proprio prezzo, quantità, fee, segnale), più **bracket SL/TP**. Quando un bracket scatta, spuntalo e si integra in una gamba di uscita.

Il modulo mostra in anteprima ingresso medio, PnL netto e quantità aperta mentre digiti. Puoi allegare fino a due immagini (screenshot del grafico), scegliere leva e moltiplicatore del contratto per i derivati, e scrivere il tuo feedback sull'operazione.

Il **PnL è calcolato in lettura** e gestisce le posizioni parzialmente aperte. La base di costo è commutabile tra **costo medio ponderato** (predefinito) e **FIFO** (utile per l'export fiscale), e la scelta è applicata davvero, sia nelle statistiche sia nell'anteprima live del PnL nel modulo.

## Fee

In **Fees & currency**, salva **strutture di fee**: fisse o percentuali, addebitate per lotto, unità, contratto o operazione (ad es. *IBKR stocks: 0,05 % per operazione*). Selezionare una struttura su un'operazione calcola automaticamente la fee; una fee inserita a mano vince sempre.

## Multi-valuta e FX

Le operazioni mantengono la valuta in cui le hai inserite. La **valuta di breakdown** (visualizzazione) viene convertita con un feed FX giornaliero che recupera automaticamente i cambi ogni giorno lavorativo, riportando i tassi in avanti su weekend e festivi.

Se un cambio non si riesce a ottenere per una data, quelle operazioni sono **escluse dai totali convertiti** e compaiono in **Pending tasks**, dove inserisci a mano i cambi mancanti basati su USD (1 USD = … di quella valuta) e le operazioni tornano a contare.

## Breakdown (le tue statistiche)

Per categoria o su tutte, filtrabili per intervallo di date, ticker, side, classe di asset, strategia, segnale e tag. La barra dei filtri è condivisa con Analytics, quindi un ambito impostato in una schermata è l'ambito dell'altra, e l'elenco dei ticker offre i simboli che il journal contiene davvero:

- **Curva di equity** nella valuta di visualizzazione.
- PnL realizzato · Rendimento · Win rate · Operazioni (chiuse/aperte) · Expectancy · Profit factor · Vincita media / Perdita media · Miglior / Peggior operazione · Max drawdown · Sharpe e Sortino · Fee totali · Capitale investito · Margine impiegato · Rendimento sul margine.
- **Sharpe e Sortino** sono calcolati sui rendimenti giornalieri rispetto all'equity portata in ogni giorno, annualizzati, con la stessa definizione usata da Analytics, così le due schermate concordano.

## Analytics (leggere il book)

L'intero book in otto schede, con gli stessi filtri del Breakdown:

- **Overview**: expectancy in **R** e R totale (sulle operazioni che portano uno stop pianificato), rischio per operazione, Sharpe con Sortino accanto, max drawdown con i giorni passati sotto il picco, giorni di trading vinti e persi, giorno medio, serie attuale e migliore, poi la **distribuzione degli R** e il costo dei tuoi errori taggati.
- **Distributions**: PnL netto per tempo di detenzione, ora di ingresso, giorno della settimana di ingresso e dimensione della posizione, e il numero di operazioni per fascia di profitto. Quale ora della tua giornata paga davvero.
- **Behavior**: cosa fai intorno al tuo vantaggio, e cosa costa. Concentrazione del profitto, dimensione dopo una serie di perdite, ritmo dopo una perdita, come decade la giornata, e l'operazione dopo una vincita contro quella dopo una perdita. Vedi [Behavior analytics](#behavior).
- **Market data**: le candele dietro le tue operazioni. MAE e MFE, efficienza di uscita, cosa è rimasto sul tavolo, distanza dello stop in ATR, e risultati divisi per regime di volatilità e per trend all'ingresso. Vedi [Arricchimento con dati di mercato](#market-data).
- **Open risk**: l'unica scheda sul presente. Cosa è ancora in gioco, dove è concentrato quel rischio, e se cinque linee aperte sono cinque scommesse o una. Vedi [Open risk](#open-risk).
- **Scatter**: due valori qualsiasi delle operazioni uno contro l'altro (data, numero operazione, netto, PnL cumulativo, rendimento sul nozionale, R, tempo di detenzione, dimensione...), colorati per risultato, side, strategia, ticker o classe di asset, con linea di tendenza e zoom.
- **Breakdown**: performance raggruppata per strategia, simbolo, tag, classe di asset o side, con operazioni, win rate, netto, expectancy, R medio e profit factor per riga.
- **Compare**: questo giorno, settimana, mese, trimestre, anno o un intervallo personalizzato contro quello appena precedente, riga per riga (netto, operazioni, win rate, expectancy, R medio, profit factor, max drawdown, fee, giorni di trading), sopra una striscia degli ultimi dodici periodi.

Le operazioni chiuse senza cambio FX per la loro data vengono contate a voce alta invece di essere scartate in silenzio.

## Behavior analytics {#behavior}

Le statistiche di performance dicono cosa ha reso il book. **Behavior** dice come ci sei arrivato, e quali tue abitudini l'hanno pagato. Stesse operazioni chiuse del resto di Analytics, stessa barra dei filtri, nessuna configurazione extra e nessun dato di mercato: legge le operazioni che hai già registrato.

Cinque schede, ciascuna risponde a una domanda.

### Da dove viene il profitto

La quota del profitto lordo fatta dalle tue cinque migliori operazioni, quante vincite servono per farne metà, e come appare l'account togliendo quelle cinque. Accanto, un indice di concentrazione: 0 significa che ogni vincita paga circa uguale, 1 che una sola operazione paga l'anno. Vincita media contro vincita mediana mostra la stessa asimmetria da un altro angolo, e una curva cumulativa la disegna.

Il numero da guardare è il netto senza le prime cinque. Se è negativo, il vantaggio poggia su outlier che non puoi programmare.

### Dimensione dopo una serie di perdite

Il nozionale mediano di ingresso raggruppato per ciò che è venuto prima dell'operazione: dopo una vincita, dopo una perdita, dopo due, dopo tre o più. Ogni riga porta il numero di operazioni, win rate, expectancy, R medio e netto, così l'escalation è prezzata, non solo notata.

Aumentare la dimensione dopo due perdite è l'abitudine più costosa che un journal intercetta. Una riga piatta qui è la disciplina che la maggior parte dei book perde per prima.

### Ritmo dopo una perdita

L'intervallo mediano da un'uscita al successivo ingresso, confrontato dopo una vincita e dopo una perdita. Un'operazione aperta in molto meno del **tuo** intervallo abituale subito dopo una perdita è contata come revenge trade, dato che uno scalper e uno swing trader non condividono un orologio. Quelle operazioni hanno una riga propria: quante, cosa hanno reso, e quanto fanno in media rispetto a tutte le altre.

Anche i giorni sono confrontati, un giorno con una perdita contro uno pulito, per numero di operazioni.

### Come va la giornata

Risultato medio per rango dell'operazione dentro il suo giorno locale: prima, seconda, terza, quarta e dopo, con R medio per rango e quanto vale ogni ulteriore operazione del giorno. Molti book fanno i soldi prima di pranzo e li restituiscono dopo. È qui che si vede.

### Dopo una vincita, dopo una perdita

L'operazione che **segue** un risultato, mai il risultato stesso. Operazioni, win rate, expectancy, R medio, dimensione mediana, rischio medio, detenzione mediana e intervallo mediano, affiancati, con i tre divari che contano (expectancy, dimensione, detenzione) evidenziati sotto.

::: tip Le affermazioni hanno una soglia minima
La frase in cima a una scheda viene scritta solo sopra una **soglia di campione** (venti operazioni chiuse nell'ambito, otto per ciascun lato di un confronto) **e** una soglia di effetto. Sotto una delle due, le schede si disegnano comunque e sono etichettate come prima occhiata. Tre operazioni non possono mostrare un'abitudine.
:::

## Arricchimento con dati di mercato {#market-data}

Il registro delle operazioni conosce il tuo ingresso, la tua uscita e il tuo stop. Non sa dove è andato il prezzo mentre eri dentro, ed è lì che stanno la maggior parte delle risposte utili: se i tuoi stop stanno dentro il rumore, quanto di ogni movimento hai davvero tenuto, e in quali condizioni di mercato la strategia funziona.

La scheda **Market data** carica le candele dietro le tue operazioni e le misura.

### Configurala una volta

Apri **Sources** nella scheda:

- **Dimensione della candela**: *Automatic* sceglie il timeframe più grossolano che lascia ancora circa venti candele dentro una posizione tipica, ricavato dal tuo tempo di detenzione mediano. Fissane uno se preferisci decidere tu.
- **Fetching**: *Off* misura solo ciò che è già salvato, *On demand* scarica quando clicchi, *Automatic* mette in coda da sola la finestra mancante di una nuova operazione e ti avvisa quando arriva.
- **Fonte per tipo di asset**: azioni, ETF, crypto, forex e future scelgono ciascuno un [data connector](/it/config/connectors) concesso al journal, oppure *Automatic*, che prende il primo connector concesso che serve quel tipo.

Nulla viene scaricato alle tue spalle, e i download sono normali job di [Historical Data](/it/modules/market-data#histdata): stessa coda, stessa contabilità delle quote, stesso elenco di job.

### Scopri, scarica, misura

Tre pulsanti, in quest'ordine.

- **Discover** legge ciò che le operazioni filtrate richiedono rispetto a ciò che hai già salvato, e non scrive nulla. Per strumento ottieni le operazioni nell'ambito, le barre in archivio, le finestre mancanti e uno stato: *ready*, *partial*, *missing*, *no source*, *unsupported*, *contract needed*.
- **Download missing** mette in coda quelle finestre e segue il batch. Si chiedono solo i buchi, e un buco è chiesto in base alle barre invece di essere indovinato da un intervallo: una serie azionaria giornaliera manca ogni weekend, una crypto 24/7 mai, quindi nessuna larghezza di intervallo funziona per entrambe.
- **Measure** percorre ogni operazione rispetto alle sue barre e salva il risultato.

La misura è **incrementale**. Una misura salvata viene rifatta quando l'operazione è stata modificata, quando sono arrivate nuove candele, o quando la granularità è cambiata. *Re-measure all* forza l'intero ambito, per quando cambi la dimensione della candela e vuoi ogni operazione sullo stesso piano.

### Nominare un contratto future

Un'azione si chiama allo stesso modo ovunque. Un contratto future no, e il ticker del tuo journal di solito nomina la radice che tratti e non il contratto che serve il tuo provider di dati.

Quindi il journal chiede una volta, invece di indovinare. Uno strumento che lo richiede mostra *contract needed*, e **Name the contract** prende il simbolo come lo scrive la tua fonte: `MNQU6` (quello che mostra TWS e che copi), oppure la radice con il mese del contratto, `MNQ.202609`, o `MNQ.202609@CME` quando la radice è quotata su più exchange. Tutto a valle usa quel simbolo.

Le opzioni sono riportate come **unsupported** invece di essere abbinate al loro sottostante. Misurare un'operazione su opzione contro le candele dell'azione produrrebbe numeri che sembrano giusti e non significano nulla.

### Cosa ottieni

- **Escursioni**: MAE e MFE medi, in denaro e in unità del rischio pianificato, con il tempo mediano dall'ingresso a ciascuno.
- **Efficienza di uscita**: la quota del miglior movimento che hai davvero tenuto, e cosa è rimasto sul tavolo su ogni operazione misurata.
- **Gli stop sono troppo stretti**: distanza mediana dello stop in ATR all'ingresso, quanti stop stanno sotto un ATR, e quante *vincite* sono prima andate oltre l'80 % del loro rischio. Uno stop dentro il rumore è uno stop che il mercato prende lungo la strada verso il tuo target.
- **I target sono troppo vicini**: vincite che hanno tenuto meno di metà del movimento offerto, e cosa si vedeva nel punto migliore rispetto a ciò che è tornato a casa.
- **Quanto in profondità prima che funzioni**: il punto peggiore di ogni operazione suddiviso in R, da 0-0,25R a oltre 1,5R. Questo ti dice dove va uno stop.
- **Per regime di volatilità** e **per trend all'ingresso**: le stesse statistiche divise calmo / normale / volatile, e in salita / piatto / in discesa.

I regimi sono **terzili del tuo book**, non soglie assolute. Una soglia assoluta chiamerebbe volatile ogni operazione crypto e non ti direbbe nulla su quando la tua strategia funziona. Sotto le dodici operazioni misurate nessun regime viene etichettato.

Anche la scheda Scatter legge questi dati: MAE contro R, efficienza contro tempo di detenzione, la nuvola colorata per regime.

## Open risk {#open-risk}

Ogni altra scheda misura il passato. **Open risk** misura ciò che è ancora in gioco adesso.

Una posizione è aperta quando resta quantità, ed è contata a quel **residuo**: un'operazione ridotta per tre quarti porta un quarto del rischio, non il rischio con cui si è aperta.

### Il quadro d'insieme

- **Esposizione lorda e netta**, in denaro e come quota dell'account, long e short sommati poi compensati.
- **In gioco**: ciò che perdi se ogni stop pianificato viene colpito. Le posizioni senza stop registrato sono contate a parte e nominate, dato che ciò che rischiano è sconosciuto, non zero.
- **Risultato aperto**, valorizzato all'ultima chiusura salvata, dicendo quante posizioni si sono potute davvero valorizzare.
- **Scommesse effettive**: a quante posizioni indipendenti corrispondono le tue linee, per dimensione, e di nuovo alle loro correlazioni misurate.

La tabella delle posizioni le elenca dalla più grande, con side, dimensione aperta, ingresso, stop, ultimo prezzo, valore, ciò che è in gioco, la sua quota del totale, risultato aperto e giorni di detenzione.

### Dove sta il rischio

Concentrazione per strumento, classe di asset, side o strategia, calcolata sul **rischio** quando gli stop sono registrati e ripiegando sulla dimensione quando non lo sono (il pannello dice quale). Uno strumento che porta metà di ciò che è in gioco è un fatto sul tuo book che nessuna curva di equity mostra.

### Sono scommesse separate

Cinque linee che si muovono insieme sono una posizione a cinque volte la dimensione. Per rispondere, la scheda misura la correlazione su **candele giornaliere già in archivio**, qualunque granularità abbia usato l'arricchimento, e non scarica mai nulla.

Tre numeri, e solo il terzo descrive il tuo book:

- **Sommati**: ogni stop colpito insieme, sommato.
- **Se indipendenti**: quale sarebbe il rischio se nulla si muovesse insieme.
- **A queste correlazioni**: ciò che il book rischia davvero.

Il loro rapporto è il fattore di **stacking**: 1,0 significa scommesse davvero separate, più alto significa la stessa scommessa più volte. Gli strumenti senza candele salvate sono nominati e lasciati fuori dalla matrice invece che presunti.

Gli avvisi si leggono come frasi: una coppia che si muove a 0,9, un singolo strumento che porta troppo, posizioni senza stop, un book più sottile di quanto sembri. Quando nulla non va, viene detto anche questo.

## Tag di disciplina

Un **tag** è una regola che hai infranto o rispettato: *ho spostato lo stop*, *nessun setup*, *ho aumentato dopo una perdita*. Spuntali sulle operazioni a cui si applicano e Analytics li prezza: quante operazioni chiuse hanno infranto una regola, cosa fanno in media rispetto a quelle pulite, e il divario tra le due. Questo è il **costo degli errori**, in denaro.

## PnL Calendar

Una griglia mensile del PnL realizzato giornaliero, verde per i giorni in positivo e rosso per quelli in negativo, scalata sul giorno più grande del mese, con i totali settimanali a lato. Clicca un giorno per saltare alle sue operazioni.

Legge anche le tue **routine di trading**. Collega le routine che una categoria segue, per un periodo, e ogni giorno di trading porta un puntino: verde quando ogni routine dovuta quel giorno è stata spuntata, rosso quando nessuna lo è stata, ambra nel mezzo. Un giorno in cui non hai fatto trading resta grigio qualunque cosa dicano le routine, e un giorno senza alcuna routine dovuta non ottiene alcun puntino. Passandoci sopra si nomina ogni routine con il proprio segno.

Le routine stesse vivono in [Trading Routines](/it/modules/productivity#routines) e si spuntano lì: il journal registra solo quali un book esegue, così la stessa abitudine non viene mai scritta o spuntata due volte.

## Importare un trade book {#import-a-trade-book}

La vista **Import** prende un trade book che tieni altrove (un export del broker, un altro journal, un foglio di calcolo) e lo trasforma in operazioni del journal. Nessun parser per broker: CSV, TSV, JSON e Parquet passano tutti dallo stesso rilevamento.

**Come funziona.** Trascina il file, il server propone una mappatura, la controlli con un'anteprima di operazioni reali, poi importi.

- Il **rilevamento** legge le intestazioni (en, fr, es, de, it, pt, più la terminologia comune dei broker) *e* i valori stessi. Delimitatore, separatore decimale e date giorno-prima vs mese-prima sono decisi per colonna. Una colonna che non riesce a identificare con confidenza resta **non mappata** invece di essere indovinata, e la assegni tu.
- **Anteprima prima di scrivere.** Il passo di analisi non scrive nulla: restituisce le operazioni costruite, i totali e gli errori per riga, e si riesegue a ogni modifica della mappatura, quindi ciò che vedi è esattamente ciò che verrà salvato. Il P&L del file stesso è confrontato con quello calcolato.
- **Forma della riga.** Una riga è o un **round trip** (una riga = un'operazione) o un'**esecuzione** (una riga = un fill). Le esecuzioni sono raggruppate per strumento in posizioni con gambe di ingresso e uscita; ciò che è ancora aperto alla fine viene importato come operazione aperta.
- **Estratti impilati.** Un export che impacchetta più tabelle in un file (lo stile IBKR) viene letto sezione per sezione, con un selettore per cambiare tabella o leggere il file piatto.
- **Point value.** Per ogni ticker trovato nel file, l'import chiede il point value del contratto, dato che nessun export lo porta. Viene salvato con la mappatura.
- **Mappature.** Salva una mappatura e il file successivo dalla stessa fonte è riconosciuto dall'impronta delle sue intestazioni e si mappa da solo. Anche le colonne che correggi a mano vengono ricordate.

::: tip Una mappatura non è un template
Un **template** del journal è il modulo con cui registri a mano un'operazione. Una **mappatura** di import dice quale colonna di un file esterno è quale campo dell'operazione. Sono elencati separatamente e mai mescolati.
:::

### Preleva da un broker

Con un [conto broker](/it/config/brokers) concesso al journal, **Preleva da un broker** fa lo stesso import senza file: scegli l'account e un periodo, e i fill tornano integrati in posizioni, in anteprima prima che venga scritto qualcosa. Non c'è mappatura da controllare, un'API risponde con campi tipizzati. Rieseguire un periodo più ampio aggiorna le posizioni già importate invece di raddoppiarle.

**Annullare un import.** Ogni import è un **batch**, elencato con la sua data, file e numero di operazioni. *Revert* elimina esattamente le operazioni che ha creato. Gli import sono anche deduplicati **per categoria**, quindi reimportare lo stesso file non cambia nulla (lo stesso estratto può comunque alimentare due categorie, dato che una categoria è un book). *Forget* un batch per togliere quella protezione e rendere di nuovo ordinarie le sue operazioni.

## Export e report

Dalla scheda Trades puoi esportare i tuoi dati e generare un report di performance:

- **Export CSV**: le operazioni grezze, per fogli di calcolo o software fiscale.
- **Report periodico**: un riepilogo di performance settimanale o mensile (win rate, expectancy, fee, ripartizione per strategia e categoria, curva di equity), reso in **Markdown o PDF**.

## Funziona con

- **Tax Calculator**: carica il PnL realizzato del journal per un anno fiscale, ripartito in guadagni capitale / derivati / crypto.
- **Historical Data**: le schede Market data e Open risk leggono le candele tramite i [data connector](/it/config/connectors) concessi al journal, e scaricano ciò che manca come normali job.
- **Trading Routines**: il PnL calendar mostra, per giorno di trading, se le routine seguite dalla categoria sono state spuntate.
- **Dashboard**: un widget quick-trade registra un'operazione dalla home.
- **RemindMe**: aggiungi promemoria collegati al journal (ad es. revisione settimanale).
