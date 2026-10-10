# Dati di mercato e backtest

Questi moduli formano una catena: **Historical Data** scarica lo storico dei prezzi in dataset locali; **Backtest** e **Quant Tools** lavorano su quei dataset, e **Visualization** disegna il grafico di qualsiasi strumento servito da un connector, salvato o no. Tutti e tre richiedono che Historical Data sia installato, dato che possiede il catalogo dataset che leggono.

## Historical Data {#histdata}

Scarica candele OHLCV da provider esterni in dataset salvati nel tuo database, oppure [importa un file](#import) che hai già. Una volta salvati, i dati sono tuoi: disegnali, fai il backtest, esportali, senza riscaricarli.

### Provider e credenziali

I provider si configurano una volta, in modo centrale, come **[data connector](/it/config/connectors)**: un account provider con nome, con le sue credenziali, un limite di richieste opzionale e i moduli autorizzati a usarlo. Historical Data non ha **impostazioni di provider proprie**: il pulsante *Connector* accanto al selettore del provider apre la stessa schermata condivisa che trovi nelle Impostazioni.

Alcuni provider sono **senza chiave** (Binance e Binance futures, Bitget, OKX, Kraken, Coinbase, Yahoo Finance) e funzionano subito; altri richiedono una chiave API, e la maggior parte ha piani gratuiti. Un connector a cui mancano le credenziali mostra *needs credentials* ed è saltato finché non le imposti.

Le chiamate in uscita sono contate in **Impostazioni → Frequenza API** così puoi osservare l'uso del piano gratuito.

### Scaricare

Scegli provider, tipo di asset, timeframe, ticker e intervallo di date, poi **Scarica**. Note:

- I **Future** usano codici contratto: base + lettera del mese + cifra dell'anno (`F G H J K M N Q U V X Z` = gen…dic), ad es. `GCJ5` per l'oro di aprile 2025.
- Le **Opzioni** sono costruite da sottostante, scadenza, call/put e strike.
- **Limiti intraday**: i provider servono la granularità intraday solo per un lookback limitato (ad es. ~7, 60 o 730 giorni a seconda del provider). Lo storico più vecchio è disponibile a **1d / 1w** senza limite. Il modulo ti avvisa prima di mettere in coda un intervallo impossibile.

::: warning Dataset scaricati prima della v0.0.15
Prima della v0.0.15, un download o un aggiornamento che arrivava all'ora corrente poteva salvare la candela ancora in formazione, e gli aggiornamenti successivi partivano dopo di essa. Un dataset così può contenere **candele incomplete** (high, low, close e volume troncati). Scaricalo di nuovo per sostituirle. Dalla v0.0.15, una candela viene salvata solo quando il suo periodo è terminato.

| Provider | Dataset che possono contenere candele incomplete |
|---|---|
| Coinbase, Kraken, Yahoo Finance, Alpaca, Massive, EODHD, Alpha Vantage, Capital.com, Interactive Brokers | scaricati o aggiornati prima della v0.0.15 |
| Binance (spot) | scaricati o aggiornati prima della v0.0.12 |
| Binance USDⓈ-M, Bitget, OKX, OANDA, TradeStation, FOREX.com | nessuno |
:::

### Più di uno alla volta

Un solo modulo mette in coda un intero batch: spunta **quanti timeframe** ti servono e digita **più ticker separati da virgole** (`BTCUSDT, ETHUSDT, SOLUSDT`). Viene messo in coda un download per ogni coppia simbolo × timeframe (3 simboli × 2 timeframe = 6 download), tutti sullo stesso connector e sullo stesso intervallo di date.

Prima di premere Scarica, il modulo **prezza il batch**: quanti download sono, circa quante richieste al provider costa, e il tempo minimo che richiederà (girano uno dopo l'altro per rispettare i rate limit del provider). Quando il connector porta un [limite di richieste](/it/config/connectors#request-limits), un piccolo indicatore mostra quanto della finestra corrente è già speso, e la riga ti avvisa quando il batch lo supera. Non è bloccato: il resto aspetta che la quota si azzeri e riprende da solo.

### Osservare i job

I download girano come **job** in background, a blocchi, con avanzamento live. Filtra i job per stato, provider, timeframe o ticker; un batch è raggruppato sotto un'unica intestazione che mostra quanti dei suoi download sono completati.

- **Un job che ha raggiunto un limite è `waiting`, non fallito.** La riga dice perché (*quota reached* o *provider rate limit*) e fa il conto alla rovescia fino al momento in cui riprende da solo.
- **Annulla** ferma qualsiasi job non finito, e un clic annulla **il resto di un batch**. L'annullamento è cooperativo: il worker si ferma al prossimo confine di blocco e le barre già scritte vengono mantenute.
- Un parcheggio lungo e la fine di un batch generano una notifica, inviata ai [canali](/it/config/settings#notifications) concessi a Historical Data.

### Dataset

La scheda **Datasets** elenca tutto ciò che è salvato: numero di barre, intervallo di date, dimensione. Da qui puoi:

- **Fetch newer**: prelevare le barre più recenti dell'ultima salvata (aggiornare un dataset).
- **Esportare** come **CSV** o **Parquet**. Il CSV si apre in qualsiasi foglio di calcolo; il Parquet è lo stesso insieme di barre tipizzato e compresso, circa un decimo della dimensione, letto da `pd.read_parquet` senza parsing di date né indovinare i dtype. Il file Parquet porta anche strumento, timeframe e fonte nei propri metadati, quindi importarlo di nuovo ovunque nell'app compila il modulo da solo.
- **Eliminare** un dataset (cancella tutte le sue barre).
- Saltare direttamente a un **grafico** di esso.

I download di Capital.com e OANDA salvano anche **bid e ask** di ogni candela; per gli altri provider un backtest li recupera quando [servono](#bid-ask-providers).

I dataset importati stanno nello stesso elenco, con il nome del luogo da cui viene il loro file e non di un provider. Non hanno il pulsante *Fetch newer*: dietro non c'è alcun provider, e il modo per estenderne uno è un altro file.

### Importare un tuo file {#import}

**Import** nella scheda Datasets legge storici di prezzo che hai già: un dump di un exchange, un export di un broker, un foglio di calcolo, un archivio di un vendor. CSV, TSV, TXT, JSON o **Parquet**, fino a 20 MB, una riga per barra.

Il file non lascia mai il tuo browser tra un passo e l'altro e non viene mai salvato sul server: ogni passo lo rispedisce, quindi non c'è un upload a metà da riprendere o ripulire.

**Le colonne sono proposte, tu le confermi.** Le intestazioni sono confrontate con un dizionario multilingue (sei lingue) *e* con l'aspetto effettivo dei valori, quindi `Date;Ouverture;Plus haut;…` e `open_time,open,high,low,close,volume` finiscono entrambi mappati. Un puntino accanto a ogni colonna dice quanto è sicuro il rilevatore; ciò di cui non è sicuro lo lascia a te. Correggere una colonna lo addestra: il file successivo con quell'intestazione si mappa da solo.

Un file Parquet è letto nella stessa griglia di un CSV, quindi rilevamento delle colonne, passo di mappatura e anteprima funzionano in modo identico. I suoi tipi sono rispettati: un timestamp INT96 legacy (quello che scrivono Spark e i pandas più vecchi) e un `DATE` diventano date, un `DECIMAL` mantiene la sua scala, un null resta una cella vuota. Lo stesso lettore serve gli import di Journal e Portfolio, quindi anche quelli accettano Parquet.

I **timestamp** sono letti come date o come interi Unix in secondi, millisecondi, microsecondi o nanosecondi, rilevati per colonna e sovrascrivibili. Un timestamp testuale che non porta fuso orario è letto con l'offset che scegli, il che decide a *quale periodo* appartiene ogni riga, non solo come viene mostrata.

**Ciò che manca è riempito, mai inventato.** Un file con una sola colonna di prezzo è una serie di close (un NAV, un livello di indice): open, high e low sono riempiti dal close, creando una barra piatta, e il modulo lo dice. Una colonna che *hai* mappato e che è vuota su una riga è un errore che nomina la sua riga, non uno zero.

Prima che venga scritto qualcosa, l'anteprima riporta su tutto il file: barre, righe, colonne, prima e ultima data, la spaziatura che i tuoi timestamp hanno davvero (proposta come timeframe), periodi mancanti a quella spaziatura, righe che condividono un periodo, barre i cui high/low non contengono open/close, e ogni riga che non si è potuta leggere.

**Ciò che il file non può dire, lo dici tu.** Un file dice "Close"; non dice che le barre sono AAPL giornaliere. Quindi l'import chiede:

- **Ticker**, **tipo di asset** e **timeframe** (il timeframe è precompilato dalla spaziatura del file).
- **Fonte**: il broker, la venue o il vendor da cui viene il file, testo libero. È *parte dell'identità della serie*, così lo stesso strumento esportato da due broker resta due dataset invece di due nastri mediati in uno.
- **Nome** e **tag**: le tue etichette, usate per ritrovare la serie nel catalogo e filtrarla.

**Importare due volte è sicuro.** Un reimport atterra sullo stesso dataset e sovrascrive periodo per periodo: stesso file, stesso risultato. I periodi sovrapposti sono contati nell'anteprima prima che tu confermi.

Un file Parquet esportato da qui salta la maggior parte di quel modulo: conosce già ticker, tipo di asset, timeframe e fonte, e compila solo le caselle che hai lasciato vuote, quindi ciò che hai digitato vince comunque. Esporta, modifica in pandas, reimporta.

Una volta importata, la serie è un dataset ordinario: backtest, Quant Tools, arricchimento del journal e i proxy fattoriali del portafoglio la leggono come una scaricata.

### Guarda prima di scaricare

Non devi mettere in coda un job per scoprire se un simbolo vale la pena di essere salvato. Il grafico preleva una finestra tramite un connector e **non salva nulla**; quando la finestra sembra giusta, **salvala** e il normale job di download viene messo in coda esattamente per quell'intervallo. Salvare sopra barre che già hai consolida invece di duplicare, quindi aggiornare un dataset dal grafico è sicuro.

## Historical Data Visualization {#histviz}

Il grafico non è legato a un dataset: apre uno **strumento**. Cerca un simbolo, scegli un timeframe, e le barre arrivano che tu le abbia scaricate o no: il server serve ciò che è già nel tuo catalogo e preleva solo i bordi mancanti tramite un [connector](/it/config/connectors). Nulla viene scritto a meno che tu non lo chieda.

La pagina è un **workspace**: una griglia di grafici, un elenco di strumenti accanto, e una sessione di quick backtest sotto. Tutto ciò che segue descrive un singolo grafico se non indicato diversamente; la griglia stessa è in [Workspace](#workspaces).

### Trovare uno strumento

Nulla viene scritto sopra le candele che non le appartenga: il **simbolo in alto a sinistra di un grafico è un pulsante**, e apre il selettore di strumenti come modale, una sola casella di ricerca su ogni connector che il grafico può usare. Digita `BTC` e Binance, Bitget, OKX, Kraken, Coinbase, Yahoo, EODHD, Alpha Vantage, Alpaca e Massive rispondono insieme, ogni risultato etichettato con il connector che l'ha servito. Filtra per tipo di asset; spunta o togli fonti nella stessa modale (la spunta **è** il permesso, e il server rifiuta un connector che questo modulo non ha mai ricevuto).

Con la casella vuota il pannello elenca ciò che hai visualizzato **di recente**, poi ciò che è già **salvato**, entrambi apribili con un clic e senza costi.

Una riga che non puoi visualizzare lo dice al posto del grafico, nominando il motivo: il connector non è mai stato concesso, il provider non conosce il simbolo, manca una credenziale, o nessun connector concesso lo serve. Ogni messaggio porta il pulsante che lo risolve.

### Workspace {#workspaces}

Un workspace è una **griglia di grafici**, da 1x1 fino a 3x4. Righe e colonne si scelgono dalla barra degli strumenti, quindi una divisione verticale, una orizzontale e una 2x2 sono lo stesso controllo e non un elenco di layout con nome; trascina i divisori per dare più spazio a un grafico, o massimizza un grafico e torna alla griglia. Tieni quanti workspace vuoi, dai loro un nome, e passa dall'uno all'altro dal selettore; quello che avevi aperto torna al ricaricamento.

Ogni grafico porta il proprio strumento, timeframe, stile di tracciato, indicatori e disegni. Un grafico si chiude dalla propria intestazione, e una cella vuota chiede uno strumento.

**Gruppi di link.** Clicca il pulsante di link di un grafico per dargli un colore. I grafici che condividono un colore condividono il **simbolo** e il **crosshair**, e anche l'intervallo visibile quando sono sullo stesso timeframe. Il timeframe stesso non è mai condiviso di proposito: tre riquadri sullo stesso simbolo a 1m, 1h e 1d sono il motivo per cui li colleghi.

**Una connessione per tutti.** I riquadri sullo stesso account condividono un unico stream live, così quattro grafici su una chiave Alpaca consumano un posto di connessione, non quattro.

### L'elenco degli strumenti {#rail}

Una barra lungo la sinistra della pagina, da due fonti che non vengono mai confuse:

- Le **liste di grafici** sono proprie della barra, create con il pulsante `+`, che apre lo stesso selettore di strumenti. Contengono coordinate di grafico, quindi un clic le visualizza senza ricerca. **Recent** è la stessa cosa senza un nome.
- Le **liste del modulo Watchlists** contengono simboli di quotazione, quindi visualizzarne uno è una ricerca. Quando la lista o la riga quota tramite un data connector, solo quel connector viene interrogato: un simbolo quotato tramite IBKR si visualizza su IBKR o per niente.

Clicca una riga per visualizzarla nel riquadro attivo, o trascinala su qualsiasi riquadro. **Una watchlist non viene mai scritta da qui**: modificarne una la copia prima in una lista di grafici e modifica la copia, e trasformare una lista di grafici in una vera watchlist è il pulsante **Promote** e nient'altro.

### Caricare la storia

Il grafico si apre sulle ultime **1500 barre** e mette un pulsante al bordo sinistro dei dati caricati. Ogni clic scorre una fetta più indietro. Nessuna richiesta al provider avviene mai senza un tuo gesto, ed è ciò che mantiene prevedibile una chiave a consumo; la scorsa si ferma quando lo storico del provider finisce.

Il menu a tendina del **timeframe** offre ogni dimensione di barra che il connector supporta, non solo quelle che hai scaricato, e cambiare timeframe **mantiene le date che stavi guardando**.

### Streaming live {#live}

Per un connector capace di streaming, il grafico **va live da solo** non appena la barra più recente
è quella che si sta formando adesso: l'ultima candela si aggiorna sul posto, e il controllo mostra lo stato
della connessione e il ritardo rispetto all'exchange. Lo stesso controllo ferma e riavvia il feed a mano.

Il live è indirizzato per **strumento**, non per dataset, e **non salva nulla**. Qualsiasi simbolo servito da un
provider di streaming può essere osservato live senza scaricarlo prima, ed è questo il punto:
puoi guardare qualcosa prima di decidere che vale la pena di tenerlo. Salva lo strumento mentre è
live e lo stesso feed inizia a registrare anche le barre chiuse nel dataset.

**Chi trasmette cosa.** Binance (spot e futures), Bitget, OKX, Kraken e Coinbase trasmettono ogni
timeframe che scaricano, giornaliero compreso, perché su un mercato 24/7 la candela giornaliera è il giorno
epoch. Anche Capital.com trasmette ogni timeframe, dalle candele bid e ask che pubblica
separatamente, disegnate al loro mid. Alpaca, Massive e
Interactive Brokers pubblicano **una sola granularità ciascuno** (barre da un minuto, aggregati da un minuto,
barre da cinque secondi) e il grafico ricava da essa il tuo timeframe. Questo copre ogni timeframe
**intraday** e lascia giornaliero e settimanale al download: una sessione azionaria non è fatta di 1440
minuti allineati all'epoch, quindi una candela giornaliera costruita così non coinciderebbe con quella salvata. Su
un timeframe che non può trasmettere, il controllo dice quali possono invece di sparire. Il
dettaglio per provider è nello [streaming live](/it/config/connectors#live-streaming).

Un grafico trasmette un solo simbolo qualunque sia il numero di riquadri: più riquadri sullo stesso strumento, e
più riquadri sullo stesso account, condividono la connessione invece di occupare ciascuno un posto.

**Quale account.** Quando un provider ha più di un connector, il controllo live acquisisce un
selettore di account. Due chiavi sono due diritti e due posti di connessione, quindi quale viene speso
è una tua scelta, non un ripiego. Cambiare riavvia il feed sull'altro.

#### Nessun buco alla giuntura

Caricare la finestra, aprire il socket e aspettare che il provider pubblichi richiedono tutti tempo,
e un provider a barre da un minuto parla solo una volta al minuto. Quando arriva il primo tick live, il
grafico può essere indietro di una o più candele, e quelle candele prima mancavano finché non ricaricavi.

Quindi lo stream dice al server dove finisce il grafico, e il server invia ciò che si è chiuso nel
frattempo: dal catalogo quando lo strumento è salvato, dal provider solo per la coda a cui
non sa rispondere, niente del tutto quando non c'è buco. Ciò che vedi andando live è ciò che ha fatto il mercato,
senza un buco nella giuntura.

#### Quando non può partire

Una chiave rifiutata, un piano senza streaming, uno strumento per cui il tuo account non ha abbonamento:
sono risposte, non guasti. Il grafico nomina quale è, cita le parole stesse del provider, e **si ferma**
invece di riconnettersi per sempre dietro un pallino che non diventa mai verde.

| Cosa vedi | Cosa significa | Cosa fare |
|---|---|---|
| *Not authorized* / chiave rifiutata | le credenziali sono sbagliate, o il piano non ha feed live (una chiave Massive gratuita scarica lo storico e viene rifiutata al login live) | correggi il connector, poi premi *Try again* |
| *No subscription* per questo simbolo | l'account è connesso ma non autorizzato al feed di quello strumento (Alpaca gratuito trasmette IEX, non SIP o OPRA; IBKR serve ciò a cui ti abboni) | scegli un altro strumento o aggiungi l'abbonamento presso il vendor |
| *Connection taken* | la maggior parte dei vendor consente una connessione live per account, e un altro programma occupa il posto | chiudi l'altro programma; questo continua a riprovare da solo, dato che si risolve da sé |
| *This timeframe does not stream* | il provider pubblica una sola granularità e il tuo timeframe è sopra di essa | passa a uno dei timeframe elencati dal controllo, o resta sui dati scaricati |
| *Connector not granted* | al grafico non è mai stato dato questo connector | concedilo in [Data connector](/it/config/connectors), dal link nel messaggio |
| *Market data lines* quasi tutte usate | Interactive Brokers limita quanti simboli un account trasmette contemporaneamente, e il workspace è vicino a quel limite | chiudi un riquadro, o ferma il feed live su uno che non guardi, prima che il grafico successivo vada muto senza motivo |

Tutto il resto (un socket caduto, un singhiozzo del provider) si riconnette in silenzio con un backoff.

### Grafico

- **Tipi di grafico**: candele, barre OHLC, linea e **Renko** (con dimensione del mattone).
- **Indicatori**: SMA, RSI, MACD e altri, come overlay o riquadri separati, ciascuno con sorgente, colori di linea/riempimento e spessore configurabili. Ogni serie ottiene **la propria riga nell'intestazione del grafico**, con nascondi, impostazioni e rimuovi al passaggio del mouse, e ogni riquadro è titolato sopra il disegno che contiene. L'intestazione **legge al crosshair**: O H L C, la variazione, e il valore di ogni indicatore sulla barra sotto il cursore, ripiegando sulla barra visibile più recente quando il cursore è lontano.
- **Impostazioni del grafico**: scala lineare o logaritmica, linee di griglia orizzontali e verticali, **separatori dei giorni**, la **chiusura precedente** disegnata come linea di riferimento, crosshair e i suoi tag di valore per serie, tooltip al passaggio (disattivato per impostazione predefinita), colori su/giù, scala dei prezzi a sinistra o a destra, e un **tag dell'ultimo prezzo** fissato sull'asse dei prezzi, tinto come la candela che l'ha prodotto.
- **La navigazione è manuale**: trascina per fare pan su entrambi gli assi (tempo di lato, prezzo su e giù), rotellina per lo zoom (calibrata per dispositivo, così un scatto del mouse e un tick del trackpad muovono la stessa quantità). Cambiare tipo di grafico mantiene lo zoom attuale.
- Il **Volume** è disegnato in fondo al riquadro del prezzo, come lo disegnano i terminali di mercato, invece che in un riquadro proprio. Più una singola modalità a schermo intero.
- I **prezzi micro-cap** sono scritti `0.0₅4549`, con il pedice che conta gli zeri, invece di una scala di etichette identiche `0.0000`.

### Confrontare due strumenti

Aggiungi un altro strumento allo stesso grafico e viene disegnato **ribasato**, dato che due prezzi in due valute su un asse non dicono nulla. Due letture, a un clic di distanza:

- **variazione percentuale**, entrambe le serie ribasate all'inizio della finestra, che risponde a *quale è salito di più*;
- **rapporto**, questo strumento diviso per l'altro, ribasato a 100, che è la vista da pair trade: la linea sale quando quello che stai visualizzando fa meglio dell'altro.

Le serie di confronto sono allineate al grafico **periodo per periodo**, così due mercati che marcano lo stesso giorno in modo diverso si allineano comunque.

### Salvare il grafico

- **Come immagine**: un PNG del grafico esattamente com'è a schermo, disegni e overlay compresi.
- **Come pagina**: un file HTML che si apre offline in qualsiasi browser, con l'immagine, di *cosa* è un'immagine (strumento, finestra, timeframe, indicatori, confronti, chi ha servito le barre), e **le barre stesse**, incorporate. Uno screenshot incollato in un documento è un'affermazione che nessuno può verificare dopo; questo può essere riletto. Nulla viene caricato: il file è costruito nel tuo browser.

### Indicatori personalizzati sul grafico {#custom-indicators-on-the-chart}

Il dialogo degli indicatori ha una seconda scheda: **Custom**. Contiene la stessa libreria a grafo di nodi da cui costruisce il modulo [Backtest](#strategies-and-custom-indicators), quindi un indicatore esiste **una volta sola** e entrambi i moduli vedono la stessa definizione. Scegline uno dall'elenco per disegnarlo, oppure costruiscine uno nuovo qui con lo stesso builder; salvare lo riscrive nella libreria condivisa.

A differenza di un indicatore del catalogo, uno personalizzato **sceglie il proprio riquadro**: sul prezzo, o in un riquadro proprio. Quella scelta è tua per istanza, quindi lo stesso indicatore può sovrapporsi alle candele su un grafico e stare sotto di esse su un altro. Un indicatore 0-100 disegnato come overlay ottiene una seconda scala nascosta, così non può appiattire il prezzo.

La definizione **viaggia con l'istanza**: un grafico disegna ancora il suo indicatore personalizzato dopo che la riga della libreria è stata eliminata, e il ricaricamento lo aggiorna dalla libreria finché la riga esiste.

### Strumenti di disegno

Una barra contro il bordo sinistro del grafico: **linea di tendenza**, linea **orizzontale** e **verticale**, **rettangolo**, **ritracciamento di Fibonacci**, **testo**, riquadri di posizione **long** e **short** (ingresso, target e stop, con il R:R risultante), e un **righello** che riporta la variazione di prezzo, la percentuale, il numero di barre e il tempo trascorso.

- Ogni oggetto ha il proprio **stile** (colore, spessore, tratteggi) ed è modificato trascinandone le maniglie.
- Un **magnete OHLC** aggancia una maniglia a open, high, low o close della barra sotto di essa, e una maniglia rilasciata vicino a un oggetto già sul grafico si aggancia a esso, con una linea guida che dice cosa ha catturato.
- **Template di stile**: dai stile a un oggetto, salvalo con un nome, e applicalo ai successivi. Un template può diventare il predefinito per ogni nuovo disegno.
- **Copia tra grafici**: <kbd>Ctrl/⌘+C</kbd> poi <kbd>Ctrl/⌘+V</kbd> incolla l'oggetto selezionato, anche su un altro strumento; <kbd>Ctrl/⌘+D</kbd> lo duplica sul posto, spostato di una barra.
- I disegni sono ancorati a **tempo e prezzo**, non a pixel, quindi restano sulle loro barre attraverso qualsiasi zoom, pan o cambio di timeframe.
- Sono conservati **per strumento**, non per timeframe e non per riquadro: una linea di tendenza disegnata sull'1h è la stessa linea sul 15m, e due riquadri sullo stesso simbolo mostrano una sola lavagna.
- **Annulla** (il pulsante della barra, o <kbd>Ctrl/⌘+Z</kbd>) riprende l'ultimo disegno, o l'ultimo ordine del quick backtest.

#### L'elenco degli oggetti

Oltre i cinque disegni un grafico ha bisogno di un elenco, ed eccolo: ogni oggetto su questo strumento con cos'è e il prezzo a cui sta, e le quattro cose che poi servono, **nascondi**, **blocca**, **elimina** e **riordina**. L'ordine è l'ordine di disegno, che decide cosa sta sopra. Selezionare una riga la seleziona sul grafico, e l'elenco è lo stesso array che il grafico disegna, quindi una modifica si vede prima che il dialogo si chiuda.

### Alert {#alerts}

Un prezzo o un livello di indicatore, **osservato dal server**. Il browser può essere chiuso, la macchina può fare altro: l'alert scatta comunque, nelle tue notifiche e nei [canali](/it/config/settings#notifications) concessi al grafico (nessuno spuntato = tutti).

Impostane uno dal dialogo degli alert, o da una linea orizzontale che hai già disegnato, che passa il suo prezzo.

Tre regole vale la pena conoscere, perché sono decisioni e non dettagli:

- **Solo barre chiuse.** L'high di una candela in formazione non è ancora un fatto, può essere rivisto dal tick successivo. Un alert scattato su di esso riporterebbe qualcosa che non è mai successo.
- **Un attraversamento, non uno stato.** *Crosses above* aspetta che il prezzo **attraversi** il livello salendo, così un alert piazzato sotto il prezzo attuale non scatta nell'istante in cui lo crei.
- **Il livello è letto sul timeframe di questo grafico**, e un alert giornaliero è riletto molto meno spesso di uno da un minuto: uno strumento non salvato costa una piccola richiesta per controllo, e una barra che si muove una volta al giorno non ne merita una al minuto.

Ogni alert può scattare **una volta** o ogni volta, con un cooldown. L'elenco dice quando ciascuno è scattato l'ultima volta, cosa ha letto, e, quando un connector o una quota si mettono in mezzo, perché non ha potuto girare. Gli alert si mettono in pausa e si riarmano da quell'elenco.

### Broker book {#broker-book}

Sincronizza un [conto broker](/it/config/brokers) e il grafico disegna ciò che detieni davvero: una linea di prezzo per posizione aperta al suo costo medio, una per ordine in lavorazione al suo limit o stop. I livelli finiscono sul grafico il cui ticker corrisponde, punteggiatura a parte, così un workspace di più strumenti si annota da solo. Sola lettura, e riletto solo quando premi *Sync*: una posizione senza costo medio non ottiene linea ed è contata come tale invece di essere piazzata in un punto plausibile.

### Quick backtest

Un blocco per fare trading a mano su un grafico: **clicca il grafico per aprire una posizione, clicca di nuovo per chiuderla**. Tieni premuto invece per scegliere il lato e la dimensione, e clicca la freccia di un marcatore per invertirlo. Pyramiding, chiusure parziali e inversioni derivano tutti dai fill che piazzi.

La sessione si estende a **tutto il workspace, non a un grafico**: ogni grafico a schermo posta i suoi fill al suo interno e i numeri ne sono la somma, come un book di più strumenti si legge davvero. Il selettore nel pannello nomina il **grafico di destinazione**, quello su cui un clic piazza un'operazione e quello che la casella della dimensione modifica; è il riquadro attivo, quindi scegliere qui e cliccare là sono lo stesso atto.

La striscia sotto il workspace è una riga di cifre di sessione quando è compressa. Espansa è ridimensionabile dal bordo superiore e ha tre schede:

- **Trades**: l'elenco delle operazioni della sessione con ingresso, uscita, P&L e R (misurato rispetto alla peggiore perdita aperta dell'operazione, dato che non c'è uno stop da citare), più un reset.
- **Statistics**: win rate, expectancy, profit factor, risultati per side, serie e una distribuzione dei rendimenti.
- **Performance**: la curva di P&L della sessione, ogni operazione con il suo run-up e la sua peggiore perdita aperta, così una vincita che ha passato la giornata in perdita si legge come tale.

La dimensione si inserisce in **unità**, **contratti** (moltiplicati per un point value) o **nozionale**, e viene convertita al prezzo di fill. Questi fill vivono **nel tuo browser, per strumento**: è un blocco per leggere un grafico, mai dati del journal, e nulla viene postato nel [Trading Journal](/it/modules/journal).

Il pulsante **Backtest** passa lo stesso strumento al modulo [Backtest](#backtest), salvandolo prima se non era salvato.

### Il grafico ricorda dove l'hai lasciato

Tipo di grafico, indicatori e disegni appartengono allo **strumento**, non a un dataset e non a un riquadro: sono salvati lato server sotto le coordinate proprie del simbolo poco dopo ogni modifica, e tornano uguali in qualsiasi riquadro, in qualsiasi workspace, da qualsiasi browser. Vale anche per un simbolo che hai guardato una volta sola e mai scaricato, ed è questo il punto: visualizzare qualcosa che non hai deciso di tenere non significa più perdere ciò che ci hai disegnato.

Ciò che *non* è salvato lato server è la sessione del quick backtest, che resta in questo browser.

L'interruttore **autosave data** nelle impostazioni del grafico è una decisione separata, sulle barre e non sul layout: salva uno strumento la prima volta che lo visualizzi, il che mette in coda un download. È disattivato per impostazione predefinita.

### Quando mancano dati

Una finestra che torna corta dice sempre **perché**, in un avviso sopra il grafico, mantenendo le barre che sono arrivate:

| Motivo | Cosa è successo |
|---|---|
| **auth** | La credenziale del connector manca o è rifiutata. |
| **quota** | Il connector ha raggiunto il proprio [limite di richieste](/it/config/connectors#request-limits). |
| **rate_limit** | Il provider ha limitato la richiesta. |
| **symbol** | Il provider non conosce questo ticker. |
| **depth** | Il provider non serve storico così indietro a questo timeframe. |
| **provider** | Qualsiasi altra cosa restituita dal provider. |

## Backtest {#backtest}

*Combina i segnali degli indicatori, dimensiona con il pyramiding, misura il vantaggio.* Scegli un dataset o un intero portafoglio, definisci le regole, esegui. Nessun codice.

### Strategia

- **Regole di ingresso / uscita** per lato, costruite da confronti tra indicatori, prezzo e valori fissi. Raggruppa le regole con **AND** (tutte devono valere) o **OR** (basta una).
- **Direzione**: long, short, o entrambe. Opzioni: derivare il lato short come specchio del long (operatori inversi, livelli degli oscillatori specchiati: RSI sotto 30 diventa RSI sopra 70; i filtri ADX, ATR e volume restano come sono), e **stop & reverse** (inverte la posizione quando scatta il segnale opposto).
- **Stop-loss / take-profit** per lato: ciascuno è una casella che attivi o disattivi in modo indipendente (percentuale dell'ingresso medio, o un multiplo dell'ATR dell'ultima candela chiusa prima dell'ingresso). Senza regole di uscita, le uscite avvengono tramite SL/TP o inversione.

### Sizing, account e costi

- Dimensiona per **percentuale dell'equity** o **quantità/lotti/contratti fissi**, con **leva** e **capitale iniziale**. La quantità fissa **scala con la leva**, seguendo la convenzione retail, quindi una leva di 3 su una dimensione fissa di 1 apre 3 unità.
- **Pyramiding**: consenti fino a N ingressi impilati quando il segnale di ingresso scatta di nuovo; SL/TP poi seguono il prezzo medio di ingresso. Un'aggiunta è inviata all'apertura, prima che la candela venga testata: una candela che poi colpisce lo stop chiude la posizione fatta dall'aggiunta, e uno stop spostato dall'aggiunta (breakeven) viene testato su quella stessa candela.
- **Costi**: fee (fissa o % del nozionale, per operazione o per unità) e **spread %**, così i risultati non sono fantasia. Una fee può essere negativa, per un rebate maker o un broker che paga per fill. La fee di ingresso esce dalla cassa al fill, come la addebita un broker, quindi equity, drawdown e aggiunte di una posizione aperta sono al netto.
- Le impostazioni senza senso (nessun capitale, una dimensione o leva pari o inferiore a zero, un contratto che non vale nulla, una griglia non negoziabile) sono rifiutate prima dell'esecuzione, e lo stesso vale per un dataset con una candela che non lo è (un high sotto il low, un prezzo che non è un numero), con quella candela nominata.

### Sizing (avanzato)

Oltre alla percentuale dell'equity e alla quantità fissa:

- **Rischio per operazione**: dimensiona in modo che uno stop-loss colpito costi una % fissa dell'equity (richiede uno stop sul lato negoziato).
- **Kelly frazionario**: dimensiona dal win rate e dal payoff delle ultime *N* operazioni di segnale della strategia, comprese quelle saltate (un'operazione in pareggio non è né vincita né perdita), scalato per la frazione scelta e con un tetto; una dimensione di warm-up è usata finché la finestra si riempie. Una fase perdente mette in pausa gli ingressi senza fermarli per sempre: riprendono quando il vantaggio torna.
- **Scaglioni di equity**: una tabella di soglie; lo scaglione più alto il cui livello è ≤ equity attuale imposta la dimensione.

### Portafoglio (multi-asset)

Aggiungi più dataset ed esegui una strategia su tutti su un **orologio unito** (tutti bloccati sullo stesso timeframe):

- Un'**anteprima dell'allineamento** mostra la lunghezza dell'orologio unito, la finestra sovrapposta, le barre di warm-up degli indicatori (compreso il lookback cumulativo di un indicatore personalizzato concatenato), e le barre mancanti per asset, tutto prima di simulare.
- **Limiti di portafoglio**: limita il numero di posizioni aperte e l'esposizione totale / per asset. Una posizione detenuta a un'apertura mantiene il suo posto lì anche se si chiude più tardi in quella candela.
- **Sessioni**: gli asset le cui candele aprono a orari diversi (un giorno crypto alle 00:00 UTC, uno di New York alle 13:30) agiscono in quell'ordine. Un ordine alla prima apertura dimensiona e controlla i suoi limiti sulla chiusura precedente dell'asset successivo, non su un'apertura che non è ancora avvenuta.
- Una **ripartizione per asset** riporta operazioni, PnL netto, fee, win rate ed esposizione per ogni strumento.

### Strategia a griglia

Una scala di livelli di prezzo tra un limite inferiore e uno superiore; ogni cella compra in basso e vende al livello successivo, **long**, **short** o **neutral**. Dimensiona una quantità fissa per livello o dividi un budget totale tra le celle, con stop opzionali sopra/sotto la scala. I risultati riportano fill, round trip e inventario finale.

- **Neutral** opera su entrambi i lati della linea centrale: le celle sotto di essa comprano e vendono un livello più su, le celle sopra vendono short e ricomprano un livello più giù. Richiede un numero dispari di livelli.
- Un acquisto riposa solo su una linea **sotto** l'ultima chiusura (una vendita sopra), e viene eseguito alla linea, o all'apertura quando la candela apre oltre di essa. I target vengono eseguiti allo stesso modo.
- Uno **stop** sotto o sopra la scala viene eseguito al suo livello (all'apertura su un gap), dopo i fill che il prezzo ha incontrato lungo la strada, poi ferma la griglia.
- Senza limiti, la scala copre l'intervallo noto finora: il minimo più basso e il massimo più alto delle candele precedenti ciascuna.
- Fuori dalla finestra di trading con *close*, l'inventario esce all'apertura, prima di qualsiasi altra cosa nella candela.

### DCA (piano di accumulo)

Una terza modalità accanto alle regole di segnale e alla griglia, per il modo in cui la maggior parte del denaro viene davvero investita: un **basket ponderato**, acquistato nel tempo, mai ribilanciato.

- **Pesi, fissi.** Ogni euro impiegato è ripartito secondo i pesi che imposti per ticker. Nulla viene ribilanciato, quindi una regola che scatta su un asset di cinque impiega la quota di quell'asset.
- **Denaro in ingresso.** Il capitale iniziale è acquistato in un colpo alla prima barra di ogni asset. Tutto ciò che segue è **denaro nuovo**: un contributo ricorrente (per barra, giorno, settimana, mese, trimestre o anno, investito all'arrivo o tenuto come cassa), e regole di acquisto che immettono un importo quando la loro condizione vale.
- **Regole di acquisto**: un importo fisso, una % della cassa, del portafoglio o della base di costo, con un numero massimo di scatti e un cooldown, eseguite all'apertura della barra successiva.
- **Regole di vendita**: una % della posizione, l'intera posizione, un numero di unità o un importo, innescate da un **obiettivo di guadagno**, una condizione, o entrambi, con i proventi tenuti come cassa o prelevati.
- Le **condizioni** sono i normali gruppi di regole del motore più due famiglie scritte per questa modalità: **metriche di mercato** (calo dal massimo, rialzo dal minimo, variazione su N barre, variazione dall'inizio) e la **posizione live** (P&L %, deriva dall'ultimo acquisto, costo medio, unità, valore, peso %, cassa %, drawdown). Sono valutate per impostazione predefinita su un **indice basket ponderato**, o per asset, che poi compra solo gli asset che reggono.
- **Misure per un piano di accumulo**, non per una strategia: drawdown e Sharpe sulla curva **corretta per i depositi (time-weighted)**, così un deposito non è letto come un rally; rendimento sul denaro immesso; **IRR** per il rendimento money-weighted; e un benchmark dello stesso totale contribuito impiegato in un'unica soluzione all'inizio.

Sizing, pyramiding e stop non si applicano qui: le regole del piano decidono ogni fill.

### Finestra di trading

Un passo **Filters** decide *quando* la strategia può aprire, sull'orologio che scegli: un fuso orario con nome (`America/New_York`), che segue l'ora legale, o un offset UTC fisso, che non la segue:

- **Giorni della settimana** e **sessioni** (più d'una al giorno, una fine prima del suo inizio supera la mezzanotte).
- **Calendario**: opera solo in date indicate, o mai in esse. *Never* vince su *only*.
- Fuori dalla finestra la posizione è o **mantenuta** o **chiusa**, e anche le aggiunte di pyramiding possono essere bloccate. Gli ingressi sono filtrati; uscite, stop e take-profit continuano a girare su ogni barra.

### Costi e realismo dell'esecuzione

- **Slippage**: un numero fisso di tick o una percentuale del prezzo, applicata a ogni fill.
- **Funding**: un tasso annuo costante sul nozionale aperto per le stime dei perp (i long pagano, gli short ricevono).
- **Circuit breaker**: ferma il trading dopo una perdita giornaliera massima (per il giorno) o un drawdown massimo (per l'esecuzione).
- **Profilo dello strumento**: tick di prezzo, passo del lotto, quantità minima e moltiplicatore del contratto, così dimensioni e prezzi si agganciano a un contratto realistico. Ogni fill e ogni stop, target, limit e linea della griglia è arrotondato al tick, contro il trader: un acquisto paga il tick sopra, lo stop di un long sta un tick più in basso.

### Esecuzione degli ordini {#execution}

Come vengono eseguiti stop e target, sempre:

- Un'operazione è testata rispetto al suo stop e take profit **sulla candela in cui si apre**, non dalla successiva.
- Il **take profit è un ordine limit**: viene eseguito al target, senza slippage né spread addebitati, una volta che il lato che tratta lo raggiunge (il bid per un long, l'ask per uno short), oppure solo quando il prezzo **attraversa** il target se scegli quello (Advanced, *Execution*). Una candela che **apre oltre il target** lo esegue a quell'apertura, prima di qualsiasi altra cosa nella candela.
- Lo **stop loss è un ordine stop**: viene eseguito allo stop, o all'apertura quando la candela fa gap oltre, e paga spread e slippage.
- Quando una candela raggiunge **sia** lo stop sia il target e nient'altro dice quale è venuto prima, vince lo stop.
- Un'operazione chiusa dentro una candela (stop, target, limit) non viene riaperta all'apertura di quella candela, un prezzo precedente all'uscita: un nuovo ingresso aspetta la candela successiva.
- Ogni segnale deciso a una chiusura viene eseguito alla **prossima apertura**: un ingresso, la condizione di uscita, e *esci quando l'ingresso non regge più*. Nulla viene eseguito alla chiusura che ha prodotto la sua stessa decisione.
- MAE e MFE contano solo ciò che l'operazione ha vissuto: dal suo fill alla sua uscita, mai il resto della candela dopo che è uscita.

Opzioni nel passo **Advanced** (*Execution*), tutte disattivate per impostazione predefinita:

- **Ordine di ingresso**: market, o **limit**. Un limit riposa sotto il riferimento per comprare e sopra per vendere, a un offset (percentuale, distanza di prezzo o multiplo di ATR) dalla chiusura della candela di segnale o dall'apertura della candela successiva. Resta valido per il numero di candele che imposti, viene eseguito al tocco o solo quando il prezzo lo attraversa, e viene eseguito al proprio prezzo senza slippage (all'apertura quando una candela apre oltre). Un segnale che continua a reggere non sposta un ordine a riposo. Le aggiunte di pyramiding seguono la stessa regola.
- **Ordine di uscita**: la stessa scelta per le uscite da segnale (la condizione di uscita, e *esci quando l'ingresso non regge più*). Un limit di uscita che non viene eseguito in tempo o **passa a market** alla prossima apertura o viene **annullato**. Uno stop-and-reverse resta un'inversione a market.
- **Controlla un timeframe inferiore per SL/TP**: quando una candela raggiunge sia lo stop sia il target, o un limit viene eseguito a metà candela, l'esecuzione legge un timeframe inferiore dello stesso strumento (stesso provider) dentro *quella sola candela* per vedere cosa è venuto prima. Legge prima un dataset salvato a quel timeframe, poi le candele scaricate da un'esecuzione precedente, che sono conservate per esecuzioni future. *Auto* prende il timeframe salvato più fine, altrimenti il più fine che il provider serve in una richiesta per candela. Le candele del timeframe inferiore che non corrispondono alla candela (estremi diversi) non vengono usate.
- **Scarica le candele mancanti del timeframe inferiore**: con questa opzione, le candele che un'esecuzione richiede e non possiede sono scaricate dal provider, e solo quelle. Dopo un'esecuzione, un avviso dice quante candele si sono risolte come stop per mancanza di esse, con il costo (richieste e tempo) e una casella per prelevarle. Un download fino a circa 4 minuti parte da solo in una finestra di avanzamento; uno più lungo aspetta te, con il rate limit del provider e la quota rimasta sul suo connector, dato che può fallire quando non ne resta abbastanza. L'esecuzione poi si ripete con le candele scaricate. Quando il provider rifiuta (nessun permesso o abbonamento per quei dati, una chiave, un rate limit) o fallisce tre volte di fila, l'esecuzione smette di chiederglielo e lo dice accanto ai risultati. Una sessione paper con l'opzione scarica le candele sotto la candela appena chiusa quando servono, aspettando un momento che il provider le pubblichi; senza l'opzione, o quando non arrivano mai, vince lo stop.
- **Usa prezzi bid/ask**: i fill leggono bid e ask invece del mid e dello spread (un acquisto a market paga l'ask, stop e target di un long scattano sul bid). Sono prelevati dal provider solo per le candele in cui può avvenire un fill, nel modo più semplice che offre (vedi la tabella sotto), e conservati per esecuzioni future. Un prelievo breve parte da solo dopo l'esecuzione, uno lungo chiede prima, con la stessa finestra di avanzamento del timeframe inferiore. Una candela senza bid/ask usa lo spread, e il risultato dice quante. I provider senza bid/ask storico lasciano l'opzione disattivata e dicono perché.
- **Fee maker separata** (nel blocco Costi): i fill limit (limit di ingresso e uscita, take profit) pagano una fee propria, negativa per un rebate.
- **Prezzi grezzi**: azioni ed ETF sono prezzati su barre rettificate per split e dividendi dove il provider le dà (Yahoo, EODHD, Alpaca; le barre di Interactive Brokers arrivano rettificate per gli split). Spunta per eseguire su prezzi come scambiati. Un dataset azionario senza serie rettificata è segnalato accanto ai risultati.

Il risultato mostra gli ordini limit piazzati, eseguiti e scaduti, quante candele avevano sia uno stop sia un target a portata e come sono state risolte (timeframe inferiore o caso peggiore), e quanti fill sono stati prezzati su bid/ask.

#### Bid/ask e prezzo live per provider {#bid-ask-providers}

| Provider | Bid/ask storico (backtest) | Prezzo live (stop, target, limit paper) | Bid/ask live (fill paper) |
|---|---|---|---|
| Capital.com | Candele bid e ask | Sì | Sì |
| OANDA | Candele bid e ask | No, controllato alla chiusura della candela | Non usato |
| Interactive Brokers | Serie di candele bid e ask | Sì | Sì |
| FOREX.com | Candele bid e ask | No, controllato alla chiusura della candela | Non usato |
| Alpaca | Quotazioni all'apertura e alla chiusura della candela (azioni, ETF, crypto) | Sì | Sì |
| Massive | Quotazioni all'apertura e alla chiusura della candela (azioni, ETF, opzioni, forex) | Sì | Sì |
| Binance, Binance Futures, Kraken, OKX, Bitget, Coinbase | Nessuno, si applica lo spread | Sì | Sì |
| TradeStation, Yahoo, EODHD, Alpha Vantage | Nessuno, si applica lo spread | No, controllato alla chiusura della candela | Non usato |

Con quotazioni lette all'apertura e alla chiusura, lo spread dentro la candela è la loro media attorno a high e low propri della candela.

#### Paper trading sul prezzo live {#paper-live}

Una sessione paper legge i suoi segnali su candele chiuse, al suo timeframe, e osserva i suoi livelli sul prezzo live nel frattempo:

- **Stop loss, take profit e ordini limit** (ingresso e uscita) scattano sul primo prezzo live che li raggiunge, senza aspettare la chiusura della candela. Il **trailing stop** si sposta ancora a ogni chiusura, e il prezzo live lo fa scattare al livello impostato allora.
- Il fill è prezzato sul bid/ask live quando la strategia usa bid/ask, altrimenti sul prezzo live con spread e slippage delle impostazioni. Viene **segnalato subito**, e l'esecuzione successiva lo riproduce sulla sua candela com'è avvenuto, senza un secondo alert.
- Un provider senza prezzo live per lo strumento è nominato nel dialogo della sessione: lì, stop, target e limit sono controllati alla chiusura della candela. Se il feed live si ferma, o l'app è offline, la candela riprende il comando e il log della sessione lo dice.
- Un ingresso è **segnalato alla chiusura che dà il suo segnale**. Un ordine a market è annunciato al prezzo di quella chiusura, con la sua dimensione (presa sull'equity valorizzata a quella chiusura) e il suo **importo** di denaro per un broker che accetta un ordine nozionale in unità frazionarie, poi viene eseguito alla prossima apertura: sul primo prezzo live quando il provider ne trasmette uno (stop e target sono poi osservati live da quel fill), altrimenti all'apertura di quella candela una volta chiusa. Il prezzo è aggiornato senza un secondo alert. Un ordine limit è annunciato quando è piazzato (*Triggering limit order*), e il suo fill è segnalato quando avviene.
- Un ingresso limit eseguito live ottiene stop e target dall'esecuzione successiva; fino ad allora quella posizione è protetta alla chiusura della candela.

Il paper trading gira lo stesso motore del backtest, con le stesse opzioni: con *Scarica le candele mancanti del timeframe inferiore* o bid/ask attivi, un'esecuzione preleva ciò che serve alla candela appena chiusa, aspettando un momento che il provider la pubblichi. La riga della sessione elenca gli ordini limit che ha in lavorazione.

### Strategie e indicatori personalizzati {#strategies-and-custom-indicators}

- **Strategie con nome**: salva, cerca, duplica e modifica configurazioni complete di strategia.
- **Versioni delle strategie** (opt-in): con il versionamento attivo in [Impostazioni → Versioni](/it/config/settings#versioning), una strategia salvata può essere versionata dal menu della cronologia nell'intestazione. Salva una versione (datata, con una nota opzionale), rivedi cosa diceva ogni passo, ripristinala (lo stato ripristinato è salvato come una nuova versione, annotata con la data della versione ripristinata, e mantiene il suo nome) o eliminala. La cronologia si apre in una finestra con una ricerca su note e date; la versione che corrisponde alla strategia attuale è segnata. Le note sono limitate a 500 caratteri. Le modifiche non salvate sono salvate prima che la versione venga presa. Disattivare il versionamento per una strategia chiede se mantenere o eliminare le sue versioni. Eliminare una strategia con versioni chiede se conservarle; quelle conservate possono ripristinarla da **Deleted strategies with versions** nella scheda Strategies. Gli agent con accesso in scrittura a Backtest possono fare lo stesso tramite [MCP](/it/config/ai-agents), salvando una versione dopo ogni aggiornamento come un commit.
- **Indicatori personalizzati**: costruisci i tuoi da passi con nome, senza codice. Ogni passo applica un indicatore integrato a una **sorgente** (un campo di prezzo o l'output di un passo precedente) oppure calcola una **formula** che referenzia i passi precedenti per nome (`@volume / SMA(@volume)`, con `+ − × ÷`, `min`, `max`, `abs`, `clamp`). Questo ti permette di concatenare indicatori: una Hull MA di un RSI, un MACD di un RSI, un rapporto di volume smussato, e così via. Gli indicatori che leggono candele intere (ATR, Stochastic, ADX, VWAP…) si applicano solo al prezzo, non a un passo derivato. I passi evidenziati sono l'output. Gli indicatori personalizzati diventano operandi nell'editor delle regole accanto a quelli integrati, e la libreria è **condivisa con il grafico**, che disegna la stessa definizione ([Indicatori personalizzati sul grafico](#custom-indicators-on-the-chart)).
- **Selettore di indicatori ricercabile**: scegli gli indicatori da un elenco raggruppato con filtro a digitazione (sia nell'editor delle regole sia nel builder degli indicatori personalizzati) invece di scorrere un lungo menu a tendina.

### Finestre di date e sweep di parametri (API)

Due funzionalità vivono sull'API invece che nel modulo. Esistono per l'[assistente](/it/modules/agent) e per chiunque pilota l'app via [MCP](/it/config/ai-agents):

- **Esecuzioni con finestra di date.** `from` / `to` su un'esecuzione (e sull'anteprima dell'allineamento) limitano l'intervallo simulato, ciò di cui hanno bisogno la validazione walk-forward e il taglio per regime: esegui 2019-2021, poi 2022-2024, e confronta. `to` include l'intero giorno.
- **`POST /api/backtest/sweep`**: esegui una griglia di parametri lato server e ottieni **ogni prova**, insieme al numero di prove. I percorsi della griglia entrano negli array (`long.entry.conditions.0.left.period`), quindi i periodi degli indicatori sono sweepabili; limite di 4 assi e 64 prove.

Uno sweep restituisce anche uno **Sharpe deflazionato**: lo Sharpe che la migliore di N strategie *senza valore* ci si aspetterebbe raggiungere, dato quanto queste specifiche prove sono variate. Confronta il vincitore con quella soglia, non con zero: su barre giornaliere reali, la migliore di otto incroci di medie mobili che segna 0,59 contro una soglia di selezione di 0,70 significa *nessuna evidenza di vantaggio*, cosa che il solo massimo avrebbe nascosto.

Restituisce anche la **probabilità di overfitting del backtest** (PBO) della griglia: le curve di equity delle prove sono tagliate in 16 blocchi, ogni metà è usata una volta come insieme in-sample e una come out-of-sample, e la PBO è la quota di divisioni in cui il vincitore in-sample si classifica nella metà inferiore out-of-sample. La scheda [Compare](#quant) di Quant calcola la stessa cifra sulle esecuzioni salvate.

### Optimizer

Prendi un'esecuzione terminata e **varia i suoi parametri**: lunghezze e soglie degli indicatori per lato, stop, sizing, costi, limiti di portafoglio, impostazioni della griglia, e quali giorni della settimana escludere (ogni sottoinsieme è provato). Ogni parametro ha un da / a / passo, e l'intestazione conta le varianti mentre le allarghi, con un tetto perché una griglia resti finita.

- **Prima che parta**, stima il costo da ciò che le esecuzioni passate hanno misurato sulla tua macchina: millisecondi per variante, worker, tempo totale. Puoi fermarti in qualsiasi momento e mantenere ciò che è stato calcolato.
- **Classifica** sulla metrica che scegli (Sharpe, Sortino, rendimento, profit factor, win rate, expectancy, max drawdown, operazioni); qualsiasi colonna riordina dopo. Con una divisione out-of-sample, ogni cifra in classifica è quella **in-sample**, e il rendimento out-of-sample è mostrato ma mai classificato: un vincitore scelto su di esso lo avrebbe visto.
- **Analisi** mostra la dispersione della metrica scelta su ogni variante: peggiore, media, migliore, e quante sono risultate positive. Un singolo buon numero vale poco se i suoi vicini sono terribili.
- **Haircut per test multipli**: misurato sulle candele all'anno che i dati hanno davvero (un mercato 24/7 ha circa cinque volte più candele orarie di un'azione), il miglior Sharpe è mostrato contro la **soglia di selezione**, lo Sharpe che la migliore di altrettante strategie *senza valore* ci si aspetterebbe raggiungere. Sotto la soglia, provare così tante varianti basta a spiegare il vincitore.
- Clicca una variante per leggere il suo **backtest completo**, riprodotto dalle sue stesse impostazioni. Nulla è salvato finché non lo **conservi** nella cronologia.

### Divisione out-of-sample

Dividi i dati in una testa **in-sample** e una coda **out-of-sample**; la strategia gira su entrambe e i due blocchi di statistiche (rendimento, profit factor, win rate, max drawdown, operazioni) sono mostrati affiancati. Un grande divario tra le colonne è un segno di overfitting.

### Paper trading

*Un'esecuzione terminata, lasciata correre in avanti.* Premi **Paper trade** su un risultato e la strategia continua a fare trading su carta, con una pianificazione, avvisando i canali che scegli. Non c'è un secondo motore: ogni esecuzione ri-simula la finestra con il normale backtest e riporta cosa è cambiato, quindi un fill paper è per costruzione il fill che il backtest avrebbe mostrato per le stesse candele.

- **La strategia è congelata** com'era eseguita, strumenti compresi. Modificare poi quella strategia non cambia una sessione in corso; il modulo della sessione offre di aggiornarla o di avviare una copia quando salvi di nuovo la strategia.
- **La prima esecuzione semina il book.** Ogni round trip già nella finestra è registrato in una volta, riassunto in un singolo evento: aprire una sessione non scatena una raffica di alert sulla storia. Solo ciò che accade dopo è un fill che merita un messaggio.
- **Finestra**: quanta storia ogni esecuzione dà al motore (candele finali, o un inizio fissato).
- La scheda **Paper** elenca le sessioni con stato, prossima esecuzione, posizioni aperte ed eventi non letti, e ciascuna può essere eseguita ora, messa in pausa, ripresa o eliminata. Il log degli eventi è conservato che sia stato inviato qualcosa o no, così ciò che non si è potuto consegnare c'è ancora quando torni.

#### Pianificazione

Ogni esecuzione, e i suoi dati, seguono l'orologio della candela stessa.

- **Intervallo** (ogni N minuti), **giornaliera**, **settimanale**, **mensile** o **una volta**, in un fuso orario reale.
- Un intervallo scatta sulla **griglia delle candele**, mai al secondo in cui la sessione è stata creata: ogni minuto a :00, ogni 15 minuti a :00 / :15 / :30 / :45, ogni ora all'ora.
- L'esecuzione **scarica la candela che aspetta** nei dataset propri della sessione. La candela in corso non viene mai salvata: ciò che viene letto è l'ultima *chiusa*, una settimana dal suo lunedì al successivo, e un giorno solo quando è passato un giorno intero dal suo stamp (una candela giornaliera USA arriva dopo le 04:00 UTC). Un'ultima candela salvata prima della fine del suo periodo viene riletta. Una candela che il provider non ha ancora pubblicato è richiesta di nuovo nel giro di un paio di secondi, e un periodo senza alcun scambio non scrive nulla, il che significa semplicemente che l'esecuzione successiva non ha nulla di nuovo da simulare.
- Un'esecuzione che non trova nuove candele non costa nulla: non simula affatto.

#### Cosa viene inviato, e dove

- **Notifica**: a ogni esecuzione, solo su un nuovo fill, o mai. *Ogni esecuzione* riporta anche quelle silenziose, l'unico modo per distinguere "non è successo nulla" da "il motore ha smesso di girare".
- **Raggruppa messaggi**: invia subito, o un digest orario, giornaliero o settimanale. Un invio raggruppato è un messaggio su un periodo, non un ping per ogni fill.
- **Avvisa su** ingressi, uscite, o entrambi, e opzionalmente solo per gli **strumenti** che nomini.
- **Canali**: i [canali di notifica](/it/config/settings#notifications) concessi a Backtest, tutti o quelli che scegli. Senza alcun canale configurato non viene inviato nulla, e ogni evento è comunque registrato nell'app.

#### Messaggi personalizzati

Tre messaggi, tre formulazioni, ciascuno con il proprio vocabolario: **Entry**, **Exit** e **Summary** (quello raggruppato). Lascia un campo vuoto e viene usata la formulazione integrata.

- Ognuno ha un **Titolo** e un **Corpo**, scritti con segnaposto:

```
{{trade.ticker}} {{trade.direction}} at {{trade.entry_price}}
```

- **Variabili** elenca esattamente ciò che quel messaggio può leggere, con un valore di esempio; cliccane una per inserirla. Un ingresso legge la metà di ingresso dell'operazione, un'uscita l'intera (P&L compreso), e il riepilogo legge il periodo, `since.*` (dall'ultimo alert), `total.*` (dall'avvio della sessione) e `open.*` (ciò che è detenuto adesso), più `session.*`, `event.*` e `stats.*`. Un percorso che un messaggio non può leggere non gli viene offerto.
- I **Filtri** si scrivono `| name:arg`, e `upper`, `lower`, `trim` e `json` non ne prendono:

```
{{trade.pnl | round:2}} {{since.from | date:YYYY-MM-DD}} {{trade.exit_reason | default:-}}
```

- L'**Anteprima** è il renderer stesso del server, quindi ciò che mostra è ciò che verrebbe inviato. Gira su un'operazione di esempio e sulle ultime cifre della sessione, quindi un valore che la sessione non ha ancora prodotto è segnato sul posto, tra parentesi, e il resto del messaggio viene comunque reso.

### Risultati

- Statistiche principali: rendimento (vs **buy & hold**), PnL netto e fee, win rate, profit factor, expectancy, max drawdown, Sharpe/Sortino.
- **Curva di equity** sovrapposta al prezzo con marcatori di ingresso/uscita.
- Un **riepilogo di performance** completo (profitto/perdita lordi, payoff ratio, maggior vincita/perdita, max vincite/perdite consecutive, barre medie in operazione…) e l'**elenco completo delle operazioni** con **MAE/MFE** per operazione (peggior perdita aperta / miglior profitto aperto mentre era in operazione), filtrabile, e motivi di uscita (segnale svanito, segnale di uscita, stop-loss, take-profit, invertita, fine dati).
- **Salva le esecuzioni** con un nome e conserva una cronologia per confrontare le strategie in seguito. Il menu **Reports** di un'esecuzione terminata la esporta per intero, per lato e per asset: un **PDF** con ogni grafico, o **Markdown** con le sole cifre. Nessuno dei due contiene l'elenco delle operazioni; il file prende il nome della strategia e del momento dell'esportazione.

## Quant Tools {#quant}

Analisi sui tuoi dataset, sui tuoi backtest salvati e, per i derivati, direttamente da un provider. Le schede sono in sei gruppi.

Quali schede richiedono cosa:

- **Asset** e **Multi-asset** leggono i dataset di [Historical Data](#histdata) salvati.
- **Strategy** legge le esecuzioni di [Backtest](#backtest) salvate.
- **Sizing** e **Calculators** prendono numeri che digiti (il volatility targeting legge anche un dataset).
- **Derivatives** legge un [connector](/it/config/connectors) Interactive Brokers o Massive concesso a Quant.

### Asset

Un dataset e una finestra temporale, condivisi da ogni scheda del gruppo.

- **Risk**: volatilità storica annualizzata, max drawdown, **Value at Risk** e **Conditional VaR** alla tua confidenza. La coda oltre il VaR è stimata in tre modi (normale, Cornish-Fisher, un fit di Pareto generalizzata delle peggiori perdite). Una tabella elenca i peggiori drawdown con profondità, date, e le barre impiegate per raggiungere il minimo e per recuperare, dato che una sola cifra di max drawdown nasconde quanto ci è voluto per colmare la buca.
- **Statistics**: che tipo di serie è.
  - Distribuzione contro una normale con la stessa media e volatilità: skew, curtosi in eccesso, un grafico QQ.
  - Dipendenza seriale: autocorrelazione dei rendimenti e dei rendimenti assoluti, con p-value di Ljung-Box. I rendimenti raramente ne mostrano, i rendimenti assoluti di solito sì: è il volatility clustering.
  - Trend o mean reversion: Hurst (R/S e DFA), variance ratio per orizzonte, e l'half-life del prezzo.
  - Stazionarietà: ADF e KPSS, letti insieme.
  - Se lo Sharpe ratio è distinguibile dalla fortuna: t-stat, Sharpe probabilistico, lunghezza minima del track record.
- **Volatility**:
  - Cinque stimatori sulle stesse barre. Close-to-close usa solo le chiusure; Parkinson, Garman-Klass, Rogers-Satchell e Yang-Zhang leggono anche il range della barra.
  - I loro percorsi rolling.
  - Un **cono di volatilità** che dice se la lettura di oggi è alta o bassa per il suo orizzonte.
  - Una previsione **GARCH(1,1)** con la sua persistenza e half-life degli shock.
- **Regimes**:
  - Un modello di Markov nascosto gaussiano divide i rendimenti in 2-4 stati, dal più calmo, e ombreggia il grafico dei prezzi per lo stato più probabile.
  - Rendimento, volatilità, tempo trascorso e permanenza tipica di ogni stato.
  - Le probabilità di transizione, e in quale stato è più probabile che sia il mercato adesso.
- **Events**: scegli una condizione (gap, grande chiusura, incrocio SMA o RSI, nuovo massimo o minimo a N barre, serie, picco di volume) e guarda cosa ha fatto il mercato dopo.
  - Rendimenti forward a più orizzonti contro la baseline incondizionata sulle stesse barre, con un p-value per orizzonte.
  - Il percorso medio intorno all'evento.
- **Seasonality**:
  - Una heatmap mese × giorno della settimana di rendimento medio, volatilità, range della barra o volume, più strisce per mese, giorno della settimana e (solo intraday) ora.
  - Ogni cella mostra il suo numero di campioni e il win rate. L'orologio orario è **UTC**.

### Multi-asset

Due o più dataset con lo stesso timeframe.

- **Portfolio**: matrice di correlazione, **frontiera efficiente** (una nuvola di allocazioni casuali; clicca il punto max-Sharpe o min-volatilità per leggere i suoi pesi) e **risk parity**.
- **Pairs**:
  - Cointegrazione (Engle-Granger, e Johansen in entrambe le direzioni) e lo spread con il suo hedge ratio, z-score e half-life.
  - Correlazione e beta rolling.
  - Correlazione lead-lag e causalità di Granger, per vedere se una serie si muove prima.
- **Basket**:
  - **PCA**: quante scommesse indipendenti contiene davvero il basket.
  - Un **dendrogramma** di correlazione: chi si muove insieme.
  - Un'allocazione **hierarchical risk parity**.
  - Una tabella di forza relativa a 1, 3, 6 e 12 mesi.
  - Uno stress test che mantiene una ponderazione attraverso ogni crisi passata che i dati coprono (2008, 2020, 2022 e altre).
- **Regression**:
  - I rendimenti di un asset su uno o più dataset di fattori (un indice, obbligazioni, oro, un settore).
  - Alpha con il suo t-stat, beta di ogni fattore, R², tracking error e information ratio.
  - Up/down capture e un beta rolling.

Mescolare classi di asset va bene, anche provider diversi: le barre sono abbinate per il **periodo** a cui appartengono, non per il timestamp con cui il provider le ha marcate. Una candela crypto giornaliera apre alle 00:00 UTC e una azionaria USA all'inizio della sessione di New York, ed entrambe sono lo stesso giorno. Ne seguono due cose, e il pannello dice quale si è applicata:

- Un basket che mescola un mercato 24/7 con uno a orario di borsa è misurato **settimanalmente**. Allineato giornalmente, il movimento di weekend dell'asset continuo finirebbe sulla stessa riga del lunedì dell'altro e sottostimerebbe quanto si muovono davvero insieme.
- L'annualizzazione è **contata sull'orologio** invece che presunta: gli stessi dataset giornalieri sono 252 periodi all'anno su una borsa e 365 su un mercato 24/7.

I dataset intraday sono l'eccezione: barre 4h ancorate a una sessione di trading e barre 4h ancorate all'orologio distano 90 minuti, quindi un basket intraday con provider misti viene rifiutato invece che approssimato. Usa dataset giornalieri, o un solo provider per l'intero basket.

### Strategy

Esecuzioni di backtest salvate. Ogni esecuzione viene riprodotta sul server per rigenerare le sue operazioni esatte.

- **Monte Carlo**: ricampiona le operazioni di un'esecuzione migliaia di volte (una per una, o a blocchi per tenere insieme le serie).
  - **Bande percentili** sul percorso dell'equity, equity finale e max drawdown.
  - La probabilità di finire in perdita.
  - Un **risk of ruin**: la quota di percorsi la cui equity è mai scesa a una soglia che imposti.
  - La vera curva di equity disegnata sopra.
- **Trades**: quanto valgono le operazioni per unità di rischio.
  - Expectancy in valuta e in **R**, la distribuzione dei multipli R, e **SQN** (al massimo su 100 operazioni, con la graduazione di Van Tharp).
  - Lo scatter **MAE/MFE**: quanto calore hanno preso le vincite, quanto lontano sono andate prima le perdite.
  - 1R è lo stop quando l'esecuzione ha uno stop percentuale, altrimenti la perdita media, e la pagina dice quale. Le esecuzioni Grid e DCA non registrano MAE/MFE, quindi lo scatter è omesso per loro.
- **Compare**: da 2 a 20 esecuzioni sulle loro date comuni.
  - Curve di equity ribasate, una tabella di rendimento, volatilità, Sharpe e drawdown, e la correlazione dei loro rendimenti.
  - Lo **Sharpe deflazionato** dell'esecuzione migliore, con le altre contate come le prove da cui è stata scelta.
  - La **probabilità di overfitting del backtest** (PBO, per cross-validation combinatoriale simmetrica su 8-16 blocchi). Una PBO sopra il 50% significa che il vincitore in-sample di solito finisce nella metà inferiore out-of-sample.

### Sizing

- **Dimensione della posizione**: dal tuo stack, ingresso, stop e rischio (percentuale o fisso), la **dimensione, nozionale, margine, esposizione e reward:risk**. Può **suggerire stop** da un dataset (volatilità, ATR, swing) e compilare l'ingresso dall'ultima chiusura.
- **Kelly**: la frazione di Kelly da win rate e payoff, con mezzo e quarto di Kelly. Il Kelly pieno massimizza la crescita di lungo periodo ma oscilla molto; la maggior parte dei trader dimensiona a metà o a un quarto.
- **Vol targeting**:
  - Mantieni volatilità target ÷ volatilità stimata dell'asset, con la stima rolling o EWMA, con tetto a una leva massima.
  - Ogni barra è dimensionata sulla stima nota prima di essa, quindi non c'è look-ahead.
  - Mostra il peso e le unità da detenere ora per la tua equity, e il track scalato contro il tenere l'asset piatto.
- **Risk of ruin**: la probabilità che un win rate, un payoff e un rischio per operazione raggiungano un dato drawdown, con un importo fisso o una frazione fissa rischiata per operazione. Tre risposte:
  - Le forme chiuse: Vince, per un importo fisso; il bound di Cramér-Lundberg, per entrambi i dimensionamenti.
  - Una simulazione sul numero di operazioni che imposti, con il suo errore standard e la curva di rovina per numero di operazioni.

### Calculators

- **Options**: prezzo e greche di Black-Scholes-Merton (vega e rho per punto, theta per giorno), un albero binomiale per l'esercizio americano con il premio di esercizio anticipato, e la **volatilità implicita** di un prezzo quotato.
- **Futures basis**: da un prezzo spot e uno future, il basis, il carry che implica all'anno, il repo implicito, il fair value al tuo tasso e rendimento, e il roll yield al contratto successivo.
- **Compounding**:
  - Dove portano un capitale e un rendimento per periodo, con contributi.
  - Il rendimento necessario per raggiungere un target, e quanti periodi ci vogliono.
  - Il guadagno necessario per risalire da un drawdown.
- **Sharpe test**: per uno Sharpe quotato senza i suoi dati.
  - È distinguibile da zero, o da un benchmark?
  - Quanto lungo deve essere il record?
  - Cosa ne resta una volta contato il numero di strategie provate (Sharpe deflazionato, con skew e curtosi)?

### Derivatives

Questi leggono direttamente il provider invece di un dataset salvato, quindi richiedono un connector **Interactive Brokers** o **Massive** concesso a Quant.

Un'esecuzione sono decine di richieste al provider, scandite dal provider. La pagina mostra le richieste fatte su quelle pianificate e cosa viene prelevato. Ciò che è stato prelevato è conservato per sei ore, quindi cambiare un tasso o una regola di roll ricalcola senza chiedere di nuovo al provider.

- **Futures curve**: i contratti con scadenza di un prodotto (`ES@CME` su Interactive Brokers, `ES` su Massive), scaduti compresi.
  - La **struttura a termine** sull'ultima sessione terminata.
  - Il **roll yield** tra il front e il contratto successivo nel tempo.
  - Una serie continua di tenere il front e fare il roll, rettificata all'indietro per rapporto così da finire al prezzo di oggi, accanto a quella giuntata che salta a ogni roll.
  - Il roll è una regola di calendario: il front è il contratto più vicino con più di *N* giorni residui.
  - Con un ticker spot (ad esempio `SPX` come indice), aggiunge il basis, il carry `ln(F/S)` all'anno, il repo implicito, e il mispricing rispetto al fair value al tasso e rendimento che inserisci.
  - Le curve Massive usano il prezzo di settlement di ogni sessione.
- **IV surface**: la catena di opzioni di un sottostante.
  - Alcune scadenze distribuite tra i giorni minimo e massimo che imposti, strike out-of-the-money su ciascun lato, ogni prezzo trasformato in una volatilità implicita Black-Scholes-Merton.
  - Smile per scadenza, la **struttura a termine ATM**, **risk reversal** e **butterfly** a 25 delta, e un controllo che la varianza totale ATM non scenda mai da una scadenza alla successiva.
  - Interactive Brokers prezza ogni opzione all'ultimo punto medio orario, preso nella stessa ora del sottostante, quindi non serve un abbonamento ai dati di mercato delle opzioni. Massive prezza ciascuna alla chiusura della sessione.
  - La pagina ti dice il numero di richieste prima di iniziare: con una chiave Massive gratuita (5 al minuto) una catena richiede diversi minuti.
- **Implied vs realized** (solo Interactive Brokers): la volatilità implicita a 30 giorni del sottostante, fino a dieci anni indietro, contro la volatilità close-to-close prima e dopo ogni giorno.
  - Il **premio di volatilità**: IV meno la volatilità che è seguita.
  - **IV rank** e **percentile** su un lookback che scegli.
  - Quanto bene l'IV ha previsto la volatilità realizzata (una regressione della realizzata successiva sull'IV).
  - La correlazione delle variazioni di IV con i movimenti di prezzo.
