# Fare trading dal grafico

Un conto broker può fare più che leggere il suo book: con il **trading** attivato, il grafico gli invia ordini, li sposta e li annulla dalla loro linea, e chiude le posizioni dalla loro. Oggi accettano ordini questi broker:

- **Interactive Brokers (Gateway)**, tramite l'IB Gateway o la TWS che esegue lei stesso. Questa pagina.
- **Binance** e **Binance USDⓈ-M Futures**, sui conti reali e sui loro ambienti di test (Spot Demo Mode, il testnet dei futures). Un conto reale accetta ordini una volta attivato il [trading reale](#live-trading) su di esso.
- **Bybit** (book spot o linear), sui conti reali e sul testnet di Bybit. Classificato **E2D**: costruito dalla documentazione di Bybit e mai testato contro Bybit; un conto reale accetta ordini una volta attivato il [trading reale](#live-trading) su di esso con l'accettazione.
- **Bybit EU** (spot), sui conti reali e sul testnet di Bybit EU. Classificato **E2D** come Bybit, con la stessa accettazione per un conto reale.
- **OKX** (spot, swap perpetui e futures a scadenza), sui conti reali e nel trading demo di OKX. Classificato **E2D**, con la stessa accettazione per un conto reale.
- **Bitget** (spot e futures USDT-M), sui conti reali e nel trading demo di Bitget. Classificato **E2D**.
- **Gate.io** (spot e perpetui USDT), sui conti reali e sul testnet di Gate.io. Classificato **E2D**.
- **KuCoin** (spot) e **KuCoin Futures**, solo sui conti reali: KuCoin non ha un ambiente di test. Classificato **E2D**.
- **Kraken** (spot), solo sui conti reali: Kraken non ha un ambiente di test spot. Classificato **E2D**.
- **Kraken Futures** (contratti multi-collaterale, PF_ e FF_), sui conti reali e sul demo di Kraken Futures. Classificato **E2D**, con la stessa accettazione per un conto reale.
- **Coinbase Advanced Trade** (spot), solo sui conti reali: Coinbase non ha un conto di test. Classificato **E2D**.
- **MEXC** (spot e perpetui USDT), solo sui conti reali: MEXC non ha un ambiente di test. Classificato **E2D**.
- **Bitfinex** (wallet exchange spot e perpetui), solo sui conti reali: Bitfinex non ha un host di test. Classificato **E2D**.
- **Deribit** (futures e perpetui: contratti lineari USDC e contratti inversi BTC ed ETH), sui conti reali e su test.deribit.com. Classificato **E2D**, con la stessa accettazione per un conto reale.
- **Paper trading**, un conto simulato gestito dall'applicazione stessa. Vedere [Paper trading](/it/config/paper-trading).

Ogni altro broker risponde che non accetta ordini da OpenTraderWorld e resta in [sola lettura](/it/config/brokers).

::: danger Sperimentale
Il trading dal grafico è **sperimentale**: funziona con Interactive Brokers, Binance, Binance USDⓈ-M Futures, Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex e Deribit, e con il paper trading. Interactive Brokers e i due conti Binance sono ampiamente testati, su mercati simulati e sugli ambienti di test dei broker. Gli altri sono costruiti dalla documentazione di ciascun broker e non sono ancora stati provati su un conto reale. Possono restare dei bug in ognuno di essi. Controlli ogni ordine presso il broker, inizi in piccolo e faccia trading a suo rischio.
:::

::: tip Conti reali
Interactive Brokers, Binance e Binance USDⓈ-M Futures sono classificati **E4**: un conto reale accetta ordini una volta attivato il [trading reale](#live-trading) su di esso, con i suoi limiti impostati. I conti paper e gli ambienti di test non ne hanno bisogno.

Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex e Deribit sono classificati **E2D**: un conto testnet o demo accetta ordini subito; un conto reale richiede il trading reale attivato con l'accettazione che il broker non è mai stato testato contro la piattaforma (vedi [Trading reale](#live-trading)).

L'annullamento degli ordini non è mai bloccato, qualunque sia il conto.
:::

## Configurazione

### 1. Il Gateway

Avvii IB Gateway (o la TWS) collegato al suo conto paper o reale e, nelle impostazioni API:

- spunti **Enable ActiveX and Socket Clients**;
- tolga la spunta a **Read-Only API**, altrimenti IB rifiuta ogni ordine;
- con Docker Engine (Linux), tolga la spunta a **Allow connections from localhost only** e aggiunga `172.28.53.10` ai **Trusted IPs**.

Il client id lo sceglie l'applicazione (da 3100 a 3115), quindi più sessioni possono condividere un Gateway.

### 2. Il conto

In **Impostazioni → Broker**, **Aggiungi conto**, scelga **Interactive Brokers (Gateway)** e compili:

| Impostazione | Valore |
|---|---|
| **Gateway host** | `host.docker.internal` se l'applicazione gira in Docker, altrimenti `127.0.0.1` |
| **Gateway port** | `4002` Gateway paper, `4001` Gateway reale, `7497` TWS paper, `7496` TWS reale |

Nessun segreto da fornire: il Gateway è già collegato. **Prova connessione** verifica che risponda.

### 3. L'interruttore di trading

Ogni riga di conto ha un chip **Trading**, accanto ai chip dei moduli. È separato di proposito:

- **Tutti i moduli** non lo include. Leggere il book e inviare ordini sono due permessi diversi.
- **Attivarlo chiede la password, ogni volta.** Disattivarlo non chiede nulla.
- I conti di paper trading fanno eccezione: nessuna password, perché non c'è denaro reale in gioco.

### 4. Il grafico

Un grafico può fare trading quando tre cose coincidono:

1. un conto è scelto nel **Broker book** del grafico;
2. il trading è attivo per quel conto;
3. i dati del grafico vengono dallo stesso broker: un conto IBKR opera su un grafico alimentato dal connettore Interactive Brokers.

Quando ne manca una, il pannello ordini dice quale e come rimediare. I future richiedono il **mese del contratto**: un future continuo viene rifiutato, quindi apra `MNQZ6` invece del grafico continuo.

## Il blocco del trading {#trading-lock}

Un secondo fattore opzionale per gli ordini. Con il blocco attivo, il primo ordine dopo l'accesso chiede il codice dell'app di autenticazione, una volta, e poi tutti i conti di trading restano aperti in quel browser fino all'uscita.

- Protegge invio, modifica e chiusura. Annullare, un ordine o tutti, non chiede mai nulla.
- Copre tutti i conti di trading tranne i [conti paper](/it/config/paper-trading).
- Lo attivi in **Impostazioni → Sicurezza → Blocco del trading**, una volta configurata l'autenticazione a due fattori. Dettagli, codici errati e ripristino: [Sicurezza dell'account](/it/config/security#trading-lock).

::: danger Togliere il secondo fattore con il blocco attivo disattiva il trading ovunque
Togliere l'autenticazione a due fattori, dalle Impostazioni o con `disable-totp`, disattiva il blocco. Se il blocco era attivo, disattiva anche il trading su **tutti** i conti, paper compresi. Lo riattivi poi conto per conto.
:::

## Inviare ordini

### Il ticket

La scheda **Ticket** del pannello ordini invia un ordine, un bracket o un OCO. **Offre ciò che accetta il broker del conto, e nient'altro**: un tipo, una validità o un'opzione che il broker non dichiara non viene mostrata (vedere [Cosa accetta ogni broker](#what-each-broker-takes)).

- **Tipi:**
  - **Market**, **Limit**, **Stop**, **Stop limit**.
  - **Trailing stop**: uno stop che segue il mercato a una distanza, in percentuale o in importo di prezzo, e non arretra mai. **Start at**, dove il broker lo prevede, lo fa attendere un prezzo prima di iniziare a seguire.
  - **MIT** (market if touched) e **LIT** (limit if touched): lo specchio di uno stop, attivato quando il mercato arriva al prezzo dal lato del limit (un acquisto sotto il mercato, una vendita sopra), poi un ordine a mercato o limit.
  - **OCO**: un limit e uno stop dallo stesso lato, dove il primo eseguito annulla l'altro. I due prezzi devono essere diversi.
- **Uscite:** gli interruttori **Take profit** e **Stop loss** aggiungono l'ordine opposto per la stessa quantità. Attivarne uno lo precompila a 0,5 % (take profit) o 0,25 % (stop loss) dall'ingresso. **Exits time in force** sceglie GTC (predefinita) o Day. Dove il broker accetta ordini reduce-only, le uscite partono reduce-only. Non disponibile su un OCO, né presso un broker che non collega uscite. Presso Interactive Brokers l'ingresso viene trattenuto finché le uscite non sono accettate, così il bracket parte intero.
- **Validità:** Day, GTC, GTD con data e ora future, IOC, FOK (tutto o niente, subito).
- **Prezzi:** come prezzo, o come distanza da bid, ask o ultimo (dall'ingresso, per le uscite), in tick quando il tick è noto.
- **Dimensione:** in **unità** (azioni, contratti), come **importo** di denaro (diviso per il valore dell'ingresso), o come denaro a **rischio** fino allo stop loss (diviso per la distanza dall'ingresso allo stop loss, che deve essere attivo). Un future conta il valore del suo punto; contratti e azioni si arrotondano per difetto a unità intere.
- **Opzioni**, ciascuna mostrata solo dove il broker la accetta:
  - **Reduce only**: riduce soltanto la posizione, non ne apre né ne inverte mai una.
  - **Post only**: un limit che resta soltanto sul book; rifiutato invece di essere eseguito subito.
  - **Iceberg**: la dimensione mostrata sul book, il resto nascosto.
  - **Trigger on**: il prezzo che uno stop o un trigger osserva (ultimo, mark, bid/ask, mid).
  - **Close position**: uno stop o MIT che chiude l'intera posizione al suo trigger, qualunque sia la sua dimensione.
  - **Outside RTH**: si attiva e viene eseguito anche fuori dall'orario regolare.
  - **Adaptive algo**: il broker lavora un ordine a mercato o limit per un prezzo migliore, paziente, normale o urgente.

Prima di inviare qualsiasi cosa, l'applicazione verifica:

- che ogni prezzo e quantità sia maggiore di zero, e che ogni opzione si adatti al tipo (post-only su un limit, un iceberg sotto la dimensione dell'ordine, una percentuale di trailing sotto 100);
- che il broker accetti il tipo, la validità e ogni opzione richiesti: ciò che non accetta viene rifiutato, nominandolo;
- che il prezzo stia sul tick dello strumento, altrimenti viene rifiutato con i due prezzi validi più vicini (`… is not on MNQZ6's 0.25 tick: use … or …`);
- che lo strumento si risolva presso il broker. **Non viene mai indovinato.**

Se una gamba di un ticket è rifiutata, l'intero ticket viene annullato.

### Gli ordini che l'applicazione segue {#followed-orders}

Ogni ordine viene salvato con il proprio id **prima** di essere inviato, e la risposta del broker viene salvata insieme a esso. Quindi:

- **Un id d'ordine viene inviato una sola volta per conto, per sempre.** Il grafico dà a ogni ordine un nuovo id; un ticket inviato di nuovo (dopo aver indicato un margine, per esempio) parte con nuovi id. Un id già usato viene rifiutato, anche dopo un riavvio.
- **Nessuna risposta non è un rifiuto.** Un ordine che il broker non conferma in tempo (10 secondi per il Gateway) viene cercato presso il broker tramite il suo id. Quando non si trova ancora, viene segnato come **sconosciuto** e **non viene mai reinviato**: i nuovi ordini su quello strumento attendono che una ricerca lo risolva. Se è ancora irrisolto dopo 5 minuti, riceve una notifica (canale di notifiche del grafico) che le chiede di verificare l'ordine presso il broker.
- **Un ordine che il broker non ha mai ricevuto** (ancora non trovato dopo le ricerche) attende lei. La scheda Ordini mostra *il broker non ha mai ricevuto questo ordine* con **Invia di nuovo**, che lo invia con nuovi id passando ogni controllo di un nuovo ticket, e **Ignora**, che non invia nulla. Riceve anche una notifica.
- **Un riavvio non dimentica nulla.** Gli ordini inviati poco prima che l'applicazione si fermasse vengono cercati all'avvio, e gli ordini che l'applicazione collega da sé (bracket Binance USDⓈ-M) vengono ripresi.

Poi l'applicazione confronta la sua vista con quella del broker, qualche secondo dopo l'avvio e a ciclo (ogni minuto finché ha ordini in corso, ogni 10 minuti altrimenti, e subito quando un flusso in tempo reale segnala eventi persi). **Vince il broker**: un ordine eseguito o annullato mentre nessuno guardava prende lo stato del broker, e un ordine inserito altrove (TWS, il sito della piattaforma) viene annotato come esterno. Una posizione cambiata senza esecuzioni che la spieghino è una **deriva**: viene scritta nel registro di audit e notificata.

### Cosa accetta ogni broker {#what-each-broker-takes}

| | Interactive Brokers | Binance spot | Binance USDⓈ-M | Paper | Bybit | Bybit EU | OKX | Bitget | Gate.io | KuCoin | Kraken | Kraken Futures | Coinbase | MEXC | Bitfinex | Deribit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **Tipi** | tutti | tutti | tutti | tutti | spot: market, limit; linear: market, limit, stop, stop limit | market, limit | mercato, limite | spot: mercato, limite, stop; futures: mercato, limite, stop, stop limite | mercato, limite | spot: mercato, limite; futures: mercato, limite, stop, stop limite | mercato, limite, stop, stop limite, MIT, LIT | mercato, limite, stop, stop limite, MIT, LIT | mercato, limite, stop limite, LIT | mercato, limite | mercato, limite, stop, stop limite | mercato, limite, stop, stop limite, MIT, LIT |
| **Validità** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK (un ordine market va IOC: FOK rifiutato) | GTC, IOC, FOK (un ordine market va IOC: FOK rifiutato) | GTC, IOC, FOK (FOK rifiutato su un ordine a mercato) | GTC, IOC, FOK (FOK rifiutato su un ordine a mercato; uno stop è GTC) | GTC, IOC, FOK (un ordine a mercato lavora IOC; futures: FOK rifiutato su un ordine a mercato) | GTC, IOC, FOK (FOK solo su un limite) | GTC, GTD, IOC, FOK (FOK solo su un limite; IOC su un limite o un ordine a mercato) | GTC, IOC, FOK (un ordine a mercato lavora IOC: FOK rifiutato; stop, MIT e LIT GTC) | GTC, GTD, IOC, FOK (un ordine a mercato lavora IOC: FOK e GTD rifiutati; uno stop limite è GTC o GTD) | GTC, IOC, FOK (FOK solo su un limite) | GTC, GTD, IOC, FOK (IOC e FOK solo su un limite; uno stop è GTC o GTD) | GTC, Day, IOC, FOK (Day rifiutato su un ordine a mercato) |
| **Uscite, OCO** | collegate da IB | liste di ordini (OTO, OTOCO, OCO) | collegate dall'applicazione | collegate dall'applicazione | linear: take profit e stop loss sull'intera posizione, a mercato | | | futures: take profit e stop loss di una posizione, spostati o tolti dal pannello ordini | | | | | | | | |
| **Trailing stop** | importo o %, dal mercato | %, a passi di 0,01 %, prezzo di partenza | % (da 0,1 a 20), prezzo di partenza | importo o %, prezzo di partenza | | | | | | | | | | | | |
| **Reduce only** | | | sì | sì | linear | | swap e futures | futures | futures | futures | | sì | | futures | perpetui | sì |
| **Post only** | | sì (limit maker) | sì (GTX) | sì | | | sì | sì | | sì | sì | sì | sì | sì | sì | sì |
| **Iceberg** | sì, unità intere | sì, limit GTC | | | | | | | | | | | | | | |
| **Trigger on** | ultimo, bid/ask, mid | | ultimo, mark | | ultimo (stop linear) | | | spot: ultimo; futures: ultimo, mark | | ultimo (stop futures) | ultimo | ultimo, mark, index | ultimo | | | ultimo, mark, index (indicato su ogni stop) |
| **Close position** | | | sì | | | | | | | | | | | | | |
| **Outside RTH** | sì | | | | | | | | | | | | | | | |
| **Adaptive algo** | sì | | | | | | | | | | | | | | | |

- **Binance spot** trattiene le uscite di un ingresso **limit** (OTO, OTOCO) e piazza quelle di un ingresso **market** una volta eseguito; un ingresso stop non accetta uscite. La metà limit di un OCO parte come limit maker. Un'uscita di vendita è per la quantità acquistata: una commissione prelevata nella moneta stessa ne lascia meno, e Binance rifiuta l'uscita.
- **Binance USDⓈ-M** non collega nulla da sé: l'applicazione invia le uscite come ordini reduce-only accanto al loro ingresso, e le collega, insieme alle coppie OCO, leggendole ogni 5 secondi. Dopo un riavvio l'applicazione riprende i collegamenti dagli ordini che ha salvato. L'hedge mode è seguito: ogni ordine indica il suo lato long o short, e un simbolo detenuto in entrambe le direzioni si chiude un lato alla volta.
- I trailing stop di **Interactive Brokers** seguono dal momento in cui vengono piazzati (nessun prezzo di partenza) e mostrano un iceberg in unità intere.

### Bybit

- **Ogni ordine va al book del suo strumento.** Un conto Bybit mostra ordini e posizioni di ogni book che legge. Un ordine va al book dello strumento del grafico: quello su cui è impostato il connettore dati Bybit del grafico (il suo **Market**: spot o linear), altrimenti l'unico book che elenca il simbolo. Un simbolo elencato da entrambi i book (`BTCUSDT`, coppia spot e perpetuo) su un grafico con dati da altrove è rifiutato con questa correzione. Il ticket offre ciò che quel book accetta.
- **Testnet.** Impostate il **Site** del conto su `testnet` (una chiave di testnet.bybit.com): i suoi ordini non muovono denaro e non richiedono il trading reale.
- **Controlli prima dell'invio.** Tick, passo di dimensione, dimensione minima e massima (limit e market separati) e valore minimo dell'ordine, dalla lista strumenti di Bybit. Un ordine rifiutato nomina la regola e i valori più vicini; nulla viene modificato. Uno strumento che ora non si negozia (prelancio, in consegna, chiuso) non accetta nuovi ordini; le cancellazioni partono comunque.
- **Modalità di posizione.** La modalità one-way o hedge è letta dal vostro conto Bybit (una volta, poi tenuta aggiornata dal flusso di Bybit) e mai cambiata. Ogni ordine linear nomina la posizione su cui agisce.
- **Stop di protezione.** Uno stop linear reduce-only parte con il *close on trigger* di Bybit: riduce la posizione anche quando manca margine, e Bybit può allora cancellare o ridurre altri ordini su quel contratto.
- **Prima del primo ordine reale di una sessione** (linear), l'ordine passa dal controllo preventivo di Bybit, che risponde il margine che prenderebbe; un rifiuto lì non invia nulla. Un primo ordine condizionato è rifiutato: inviate prima un ordine market o limit semplice.
- **Aggiornamenti in diretta.** Gli ordini seguono il flusso privato di Bybit; le posizioni sono rilette quando cambiano. Se il flusso non è disponibile (una chiave RSA, un sito regionale senza flusso), il conto è letto a cicli.
- **Se l'app si ferma**, Bybit mantiene attivi i vostri ordini: nulla li cancella.
- **Take profit e stop loss (linear).** Take profit e stop loss di un ticket partono con l'ingresso in una sola chiamata e coprono l'intera posizione (modalità *Full* di Bybit): ciascuno parte come ordine a mercato quando il suo prezzo viene scambiato. Bybit li crea quando l'ingresso viene eseguito; compaiono nel pannello ordini come ordini take profit e stop loss collegati all'ingresso finché lavora. Spostarli o rimuoverli li cambia sulla posizione. Un take profit limit parte a mercato, non come limit.
- **Non ancora offerti:** take profit e stop loss su spot o su parte della posizione, OCO, modifica di un ordine sul posto, trailing stop, post only.

### Bybit EU

- **Solo spot**, attraverso lo stesso percorso degli ordini di Bybit spot: ordini market e limit, gli stessi controlli prima dell'invio, un ordine market dimensionato nella moneta base.
- **Testnet.** Impostate il **Site** del conto su `eu-testnet` (una chiave di testnet.bybit.eu): i suoi ordini non muovono denaro e non richiedono il trading reale.
- **Aggiornamenti in diretta.** Bybit non documenta alcun flusso privato per Bybit EU: il conto è letto a cicli.
- **La vostra chiave.** La guida di Bybit dice che l'API del sito EU serve le applicazioni di terze parti degli utenti API broker. Una chiave personale può quindi essere rifiutata: l'ordine mostra il rifiuto di Bybit con il suo codice, e nient'altro viene inviato.

### OKX

- **Book.** Spot, swap perpetui e futures a scadenza. Ogni ordine va al book del suo strumento: `BTC-USDT` è spot, `BTC-USDT-SWAP` uno swap, `BTC-USDT-251226` un future a scadenza.
- **Regione.** Impostate la **Regione** del conto su quella in cui è stato aperto: `global` (okx.com, predefinita), `eea` (my.okx.com) o `us` (app.okx.com, Stati Uniti e Australia). Una chiave funziona solo nella sua regione.
- **Trading demo.** Impostate **Ambiente** su `demo` con una chiave di trading demo: i suoi ordini non muovono denaro e non richiedono il trading reale.
- **Dimensioni.** Swap e futures si negoziano in contratti: una dimensione che non è un numero intero di contratti viene rifiutata, indicando le dimensioni più vicine.
- **Modificare un ordine** sul posto: il suo prezzo e la sua dimensione.
- **La modalità di posizione** (net o long/short) è letta dal vostro conto e mai cambiata. La leva è mostrata, mai impostata. Gli ordini spot usano solo le vostre monete, mai margine.
- **Non offerto:** ordini stop e a trigger, take profit e stop loss, trailing stop, OCO, iceberg, opzioni, contratti inversi.

### Bitget

- **Book.** Spot e futures USDT-M. Un ordine va al book impostato sul connettore dati Bitget del grafico (il suo **Mercato**: `spot`, o `usdt-futures`, scritto anche `futures`), altrimenti all'unico book che elenca il simbolo. Un simbolo elencato da entrambi i book, su un grafico i cui dati vengono da altrove, è rifiutato con questa soluzione.
- **Trading demo.** Impostate **Trading demo** su `yes` con una chiave demo di Bitget: i suoi ordini non muovono denaro e non richiedono il trading reale.
- **Gli stop** partono come ordini a trigger di Bitget, sull'ultimo prezzo (futures: ultimo o mark). Uno stop spot si esegue a mercato.
- **Un acquisto spot a mercato** è un importo da spendere nella valuta di quotazione: l'applicazione legge il miglior ask appena prima dell'invio e invia quell'importo, quindi la dimensione eseguita può differire un poco. La risposta lo dice.
- **Tutti gli ordini del conto** compaiono nel pannello ordini, ordini a trigger compresi, e si annullano da lì.
- **Take profit e stop loss di una posizione (futures).** Quello impostato su una posizione compare nel pannello ordini; spostarlo o toglierlo lì lo cambia sulla posizione.
- **La modalità di posizione e il margine** sono letti dal vostro conto e mai cambiati. La leva è mostrata, mai impostata.
- **Non offerto:** modificare un ordine sul posto, un take profit e stop loss inviati con l'entrata, trailing stop, stop limite spot, OCO.

### Gate.io

- **Book.** Spot e i perpetui USDT, scelti come per Bitget: il **Mercato** del connettore dati del grafico (`spot`, o `futures`, scritto anche `usdt-futures`), altrimenti l'unico book che elenca il simbolo.
- **Testnet.** Impostate il **Site** del conto su `testnet` con una chiave testnet: i suoi ordini e le sue letture vanno al testnet di Gate.io, non muovono denaro e non richiedono il trading reale.
- **Un acquisto spot a mercato** è un importo da spendere nella valuta di quotazione, valutato al miglior ask letto appena prima dell'invio e arrotondato per difetto alla precisione della coppia. La dimensione eseguita può differire un poco; la risposta lo dice.
- **Le dimensioni futures** sono contratti interi: una dimensione che non lo è viene rifiutata, indicando le dimensioni più vicine.
- **Modificare un ordine** sul posto: in spot il prezzo o la dimensione (uno alla volta), in futures il prezzo e la dimensione.
- **Non offerto:** stop, take profit e stop loss, trailing stop, post only, OCO, iceberg.

### KuCoin e KuCoin Futures

- **Nessun ambiente di test.** KuCoin non ne ha: ogni conto è reale e richiede il trading reale attivato con l'accettazione. Prima del primo ordine di una sessione, l'applicazione lo invia al test ordini di KuCoin, che lo verifica senza piazzarlo.
- **Spot:** ordini a mercato e limite, dimensionati nella valuta base.
- **Futures:** mercato, limite, stop e stop limite sull'ultimo prezzo, reduce only, post only. Le dimensioni sono contratti interi. Gli stop compaiono nel pannello ordini e si annullano da lì.
- **La modalità di posizione e di margine** è letta dal vostro conto e mai cambiata. La leva è mostrata, mai impostata.
- **Non offerto:** modificare un ordine sul posto, un take profit e stop loss inviati con l'entrata, stop spot, OCO, futures con margine in moneta, trading a margine.

### Kraken

- **Nessun ambiente di test.** Kraken non ne ha per lo spot: ogni conto è reale e richiede il trading reale attivato con l'accettazione. Prima del primo ordine di una sessione, l'applicazione lo invia alla chiamata di validazione di Kraken, che lo verifica senza piazzarlo.
- **La vostra chiave.** Datele Query Funds, Query Open Orders & Trades, Query Closed Orders & Trades, Create & Modify Orders, Cancel/Close Orders e Access WebSocket API, mai Withdraw Funds. Date all'applicazione una chiave tutta sua: Kraken rifiuta una chiamata il cui nonce non è superiore all'ultimo della chiave, quindi una chiave usata anche da un altro programma fallisce a caso.
- **Dimensioni e commissioni.** Gli ordini sono dimensionati nella moneta base; le commissioni sono prelevate nella valuta di quotazione.
- **Modificare un ordine** sul posto: il suo prezzo, il trigger e la dimensione. Annulla tutti è una sola chiamata.
- **Non offerto:** margine, un ordine di chiusura collegato all'entrata, trailing stop, iceberg, trigger sul prezzo index, azioni tokenizzate.

### Kraken Futures

- **Contratti.** Gli ordini vanno sui contratti multi-collaterale (PF_, FF_), dimensionati nella moneta. I contratti inversi (PI_, FI_) compaiono con i loro ordini e si annullano dal pannello ordini, ma non accettano nuovi ordini.
- **Demo.** Impostate l'**Ambiente** del conto su `demo` con una chiave del demo di Kraken Futures: i suoi ordini non muovono denaro e non richiedono il trading reale.
- **La vostra chiave.** General API: Full Access per inviare ordini; una chiave Read Only legge comunque. Non concedete mai la Withdrawal API.
- **Modificare un ordine** sul posto: la sua dimensione, il limit e il trigger.
- **Tutti gli ordini del conto** compaiono nel pannello ordini, ordini a trigger compresi, e annulla tutti li copre.
- **La modalità di posizione** è one-way. La leva è mostrata, mai impostata.
- **Aggiornamenti in diretta.** Nessun flusso privato è usato: il conto è letto a cicli. La commissione di un'esecuzione è mostrata senza la sua valuta, che Kraken non indica.
- **Non offerto:** trailing stop, un take profit e stop loss inviati con l'entrata, OCO, opzioni, ordini sui contratti inversi.

### Coinbase

- **Nessun conto di test.** Coinbase non ne ha: ogni conto è reale e richiede il trading reale attivato con l'accettazione. Prima del primo ordine di una sessione, l'applicazione lo invia all'anteprima ordini di Coinbase.
- **Solo spot**, dimensionato nella moneta base. Uno stop limite scatta sull'ultimo prezzo.
- **Controlli prima dell'invio.** Tick, passo di dimensione, dimensione minima e massima, dalla lista prodotti di Coinbase. Un prodotto in modalità solo limit o solo post only accetta solo quegli ordini.
- **Modificare un ordine** sul posto: il prezzo e la dimensione di un limit.
- **Tutti gli ordini del conto** compaiono nel pannello ordini, compresi gli ordini piazzati nell'app Coinbase.
- **La vostra chiave.** Serve il permesso Trade per inviare ordini; una chiave View-only legge comunque.
- **Aggiornamenti in diretta.** Gli ordini seguono il flusso utente di Coinbase; le esecuzioni sono lette a cicli. Un'esecuzione che Coinbase ha rettificato (storno, correzione) è esclusa dall'importazione e indicata nel log del server; le altre esecuzioni sono conservate.
- **Se l'app si ferma**, Coinbase mantiene attivi i vostri ordini.
- **Non offerto:** stop market, take profit e stop loss, trailing stop, OCO, futures e perpetui.

### MEXC

- **Nessun ambiente di test.** MEXC non ne ha: ogni conto è reale e richiede il trading reale attivato con l'accettazione. Prima del primo ordine spot di una sessione, l'applicazione lo invia al test ordini di MEXC.
- **Book.** Spot e i perpetui USDT, scelti come per Bitget: il **Mercato** del connettore dati del grafico, altrimenti l'unico book che elenca il simbolo.
- **Un acquisto spot a mercato** è un importo da spendere nella valuta di quotazione, valutato al miglior ask letto appena prima dell'invio e arrotondato per difetto alla precisione della coppia.
- **Le dimensioni futures** sono contratti interi. I nuovi ordini partono solo in hedge mode: su un conto one-way sono rifiutati prima che parta qualcosa (passate il conto in hedge mode su MEXC), mentre una posizione si chiude comunque. Modalità di margine e leva sono lette, mai impostate.
- **Tutti gli ordini del conto** compaiono nel pannello ordini, compresi i piani futures, take profit e stop loss, e gli ordini trailing piazzati altrove, e annulla tutti li copre.
- **La vostra chiave.** Trading spot e piazzamento ordini futures, e ogni coppia spot aggiunta alle coppie di trading della chiave; il piazzamento ordini futures richiede il KYC.
- **Non offerto:** piazzare stop, take profit e stop loss o trailing stop, ordini chase, modificare un ordine sul posto, OCO, futures con margine in moneta.

### Bitfinex

- **Nessun host di test.** Bitfinex non ne ha: ogni conto è reale, sotto-conti paper compresi, e richiede il trading reale attivato con l'accettazione.
- **Book.** Le coppie spot del wallet exchange e i perpetui (`BTCF0:USTF0`). Un ordine va al book impostato sul connettore dati Bitfinex del grafico (il suo **Mercato**: spot, o derivatives, scritto anche perp o futures), altrimenti all'unico book che elenca il simbolo. Il trading a margine non è offerto.
- **La leva** è quella della posizione detenuta, inviata solo se è un numero intero da 1 a 100; senza posizione, Bitfinex applica il suo valore predefinito (10). L'applicazione non la sceglie mai.
- **I prezzi** portano al massimo 5 cifre significative: un altro prezzo è rifiutato con i due più vicini.
- **Modificare un ordine** sposta solo i suoi prezzi: per cambiarne la dimensione, annullatelo e piazzatene uno nuovo.
- **La vostra chiave.** Accesso in scrittura a Orders per inviare; una chiave di sola lettura legge comunque. Date all'applicazione una chiave tutta sua (Bitfinex controlla un nonce crescente per chiave).

### Deribit

- **Book.** Futures e perpetui: contratti lineari USDC dimensionati nella moneta, contratti inversi BTC ed ETH dimensionati in USD. Su un contratto inverso il ticket accetta la dimensione nella moneta o in USD: una dimensione in USD è arrotondata per difetto al passo del contratto (10 USD su BTC), una dimensione in moneta è convertita all'ultimo prezzo, poi arrotondata per difetto. Posizioni e ordini sui contratti inversi compaiono in USD. I vostri limiti di rischio valutano un ordine dato in USD a quell'importo.
- **Ambiente di test.** Impostate l'**Ambiente** del conto su `test` con una chiave di test.deribit.com: i suoi ordini non muovono denaro e non richiedono il trading reale.
- **Gli stop** richiedono il loro trigger (ultimo, mark o index): non c'è un valore predefinito.
- **Post only** è rifiutato invece di essere spostato quando verrebbe eseguito subito.
- **Modificare un ordine** sul posto: il suo prezzo, il trigger e la dimensione. Uno stop scattato è seguito fino all'ordine in cui si è trasformato.
- **Tutti gli ordini del conto** compaiono nel pannello ordini, ordini a trigger compresi, e annulla tutti li copre.
- **Una posizione per strumento.** La leva è mostrata, mai impostata.
- **Non offerto:** opzioni, spot, un take profit e stop loss inviati con l'entrata, OCO, trailing stop, iceberg.

### Binance

- **Chiave API.** Gli ordini richiedono una chiave con il trading abilitato («Enable Spot & Margin Trading» per lo spot, «Enable Futures» per USDⓈ-M) e i prelievi disattivati. Su un conto reale, prima del primo ordine di una sessione, l'applicazione legge i diritti della chiave e rifiuta di inviare quando il trading è disattivato, i prelievi sono attivi o la chiave non è limitata a IP attendibili. Poi esegue l'ordine in prova sulla rotta di test di Binance.
- **Controlli prima dell'invio.** Ogni ordine è sottoposto ai filtri del simbolo come li pubblica Binance: tick, passo e limiti di dimensione, dimensione degli ordini a mercato, valore minimo e massimo dell'ordine, la banda di prezzo attorno al prezzo di riferimento corrente (mark price sui futures) e il tetto di ordini aperti per simbolo. Un ordine rifiutato nomina il filtro e i valori più vicini che passerebbero. Nulla viene cambiato per lei.
- **Frequenza degli ordini.** Quando il limite di ordini del conto verrebbe superato, l'ordine viene rifiutato indicando i secondi da attendere. Non viene mai messo in coda.
- **Modificare uno stop dei futures.** Binance non può modificare un ordine condizionale sul posto. L'applicazione piazza prima lo stop modificato e poi annulla il vecchio, così uno stop è sempre attivo. L'ordine riceve un nuovo id (`….r1`).
- **Se l'applicazione scompare (USDⓈ-M).** Finché ha ordini attivi su un simbolo dei futures, l'applicazione mantiene armato il conto alla rovescia di annullamento automatico di Binance (120 s, rinnovato ogni 30 s). Se smette di rinnovarlo, Binance annulla tutti gli ordini aperti su quel simbolo, compresi quelli inseriti altrove.
- **Posizioni.** Le posizioni dei futures mostrano la leva e la modalità di margine (cross o isolated) impostate presso Binance. L'applicazione le legge soltanto.
- **Scheda Conto (spot).** La liquidità è il saldo in USDT; il disponibile ne è la parte libera. L'equity è il valore che Binance dà al wallet spot in USDT, solo su reale (Spot Demo Mode non lo pubblica).

### Interactive Brokers

- Una dimensione che il contratto non accetta (sotto il suo minimo, o fuori dal suo passo di dimensione, per esempio 0,5 di un future) viene rifiutata prima dell'invio, con le dimensioni valide.
- Un annullamento che arriva dopo l'esecuzione dell'ordine non è un errore: l'ordine risulta eseguito.
- Finché IB segnala di aver perso i suoi server (1100), i nuovi ordini e le modifiche vengono rifiutati; gli annullamenti passano comunque.
- Su un conto reale, il primo ordine di ogni connessione al Gateway viene inviato prima a IB come what-if. Se IB lo rifiuta, o dice che il conto non può sostenerlo, il ticket viene rifiutato con il motivo indicato da IB.

Il ticket si aggancia in basso, in alto, a sinistra o a destra del grafico, oppure tra righe o colonne di un layout. Trascini il suo bordo interno per ridimensionarlo (funzionano anche le frecce, Maiusc per passi più ampi); la dimensione è ricordata per posizione.

### La barra rapida

La **barra rapida Compra/Vendi** (pulsante della barra degli strumenti) mostra Vendi, una quantità e Compra con bid e ask. **Un clic invia subito un ordine a mercato, senza conferma.** La trascini dove vuole; si aggancia al bordo più vicino.

### Il `+` sulla scala dei prezzi

Passi sulla scala dei prezzi e clicchi **+**: il menu propone un acquisto o una vendita a quel prezzo, come limit o stop a seconda del lato del mercato in cui si trova. Parte subito, Day, oppure si apre nel ticket con **Apri nel ticket…**.

## Sul grafico

- **Gli ordini attivi** sono disegnati come linee. Trascini l'etichetta di una linea per spostare l'ordine (si allinea al tick); uno spostamento rifiutato rimette la linea al suo posto. La sua **✕** annulla l'ordine e le sue uscite.
- **Doppio clic** su una linea d'ordine, o la scheda Ordini, lo apre nel ticket per cambiarne prezzi, quantità o validità. Lato e tipo non cambiano. Anche gli ordini inviati dalla TWS compaiono e si modificano allo stesso modo se sono market, limit, stop o stop-limit.
- **Le posizioni** sono disegnate al loro prezzo medio, con una **✕** per chiudere a mercato. La chiusura invia un ordine a mercato per la quantità che il broker detiene ora, poi **annulla le uscite della posizione**: gli ordini attivi sul suo strumento, dal lato della chiusura, che il broker collega a un ingresso o contrassegna reduce-only. Gli altri ordini sullo strumento restano.
- **Proteggi con uscite** (lo scudo nella scheda Posizioni) apre il ticket sul grafico della posizione con un OCO dall'altro lato per la quantità detenuta: un take profit e uno stop loss, uno che annulla l'altro, reduce-only dove il broker lo prevede. La posizione deve essere aperta su un grafico di quel conto.
- **Bracket:** passando su un ordine di un bracket si disegnano i suoi collegamenti, tratteggio verde verso il take profit e rosso verso lo stop loss.
- **Esecuzioni:** con **Marcatori di entrata e uscita** attivo, le esecuzioni del conto sullo strumento sono disegnate come marcatori al loro prezzo e alla loro barra (un acquisto punta in alto, una vendita in basso), sugli ultimi 30 giorni al massimo. È disattivato per impostazione predefinita.

**Impostazioni del grafico → Trading** stabilisce come vengono disegnati:

| | Opzioni |
|---|---|
| **Posizione** | Risultato sulla linea (nessuno, percentuale, importo), Linea (larghezza piena, corta, solo etichetta sulla scala dei prezzi), mostra il lato, mostra la quantità |
| **Ordine** | Linea, mostra il lato, mostra la quantità |
| **Esecuzioni** | Marcatori di entrata e uscita, colori del Marcatore di acquisto e del Marcatore di vendita |

## Il pannello ordini

Accanto a **Ticket**, filtrato per broker:

| Scheda | Mostra |
|---|---|
| **Posizioni** | simbolo, lato, quantità, prezzo medio, prezzo di mercato, P&L, ora di apertura e commissioni d'ingresso (dalle esecuzioni degli ultimi 7 giorni), con pulsante di chiusura |
| **Ordini** | tipo, quantità, limit, stop e stato, con modifica e annullamento. Il suo **Annulla tutti** annulla gli ordini elencati in questo browser |
| **Conto** | i valori che il broker comunica, vedere sotto |
| **Log** | le ultime 200 chiamate inviate al broker, dal registro di audit |

### Valori del conto

**Equity, Liquidità, Risultato aperto, Realizzato, Margine iniziale, Margine di mantenimento, Disponibile, Liquidità in eccesso, Potere d'acquisto, Valore delle posizioni**, nella valuta base del conto, una riga per ogni conto raggiunto dal Gateway. Per Interactive Brokers sono quelli di IB (`NetLiquidation`, `TotalCashValue`, `UnrealizedPnL`, il `RealizedPnL` di oggi, `InitMarginReq`, `MaintMarginReq`, `AvailableFunds`, `ExcessLiquidity`, `BuyingPower`, `GrossPositionValue`). Un valore che il broker non comunica non viene mostrato: IB non fornisce un totale delle commissioni, quindi quella riga manca.

### Aggiornamenti in tempo reale

Ordini, esecuzioni e posizioni arrivano come li comunica il broker, su un unico flusso per tutti i conti di trading della pagina: i messaggi propri del Gateway per Interactive Brokers, lo user data stream di Binance per Binance (l'API WebSocket sullo spot, la listen key del conto su USDⓈ-M, entrambe con la chiave già configurata del conto). Le posizioni dei futures in hedge mode vengono rilette per intero invece di essere aggiornate sul posto. Il book viene anche riletto ogni 30 secondi, e un secondo dopo che una posizione si muove. Se il flusso cade, la pagina passa a leggere il book ogni 4 secondi. Un Gateway disconnesso viene riconnesso alla chiamata successiva; nel frattempo gli ordini attivi restano presso IB.

## Limiti di rischio e interruttore di emergenza {#risk-limits}

I vostri limiti, verificati prima che parta qualsiasi ordine, su ogni conto di trading (paper incluso). Ognuno è disattivato finché non lo impostate, in **Impostazioni → Broker → Limiti di rischio** del conto:

| Limite | Rifiuta |
|---|---|
| **Ordine più grande (dimensione)** | un ordine più grande di questo numero di unità |
| **Valore massimo di un ordine (valuta del conto)** | un ordine che vale più di questo, nella valuta del conto (dimensione × prezzo × valore del punto di un future) |
| **Posizione più grande (dimensione)** | un ordine che lascerebbe più di questo numero di unità detenute sullo strumento |
| **Valore massimo di una posizione (valuta del conto)** | un ordine che lascerebbe una posizione che vale più di questo, nella valuta del conto (dimensione detenuta × prezzo × valore del punto di un future) |
| **Banda di prezzo (%)** | un limit o stop oltre questa percentuale dall'ultimo prezzo del grafico |
| **Ordini al minuto** | più ordini di questo numero nell'ultimo minuto |
| **Ordini aperti contemporaneamente** | più ordini attivi di questo numero |
| **Limite di perdita per periodo** | qualsiasi ordine una volta che il conto ha perso questo importo dall'inizio del periodo (ora, giorno o settimana, vedi sotto): il conto viene sospeso. La sua unità è un **Importo** nella valuta del conto o una **% del capitale** con cui il periodo è iniziato |
| **Finestra dei duplicati (secondi)** | lo stesso ordine (lato, tipo, dimensione, prezzi) inviato di nuovo entro questi secondi, finché non confermate |

- Una violazione viene rifiutata indicando limite e valore. **L'ordine non viene mai ridimensionato per rientrare.**
- **Ridurre il rischio passa sempre**: un ordine reduce-only, una chiusura o un ordine che riduce soltanto ciò che è detenuto supera ogni limite e ogni sospensione.
- Modificare un limite richiede la vostra password.

### Sospensioni

Una sospensione rifiuta i nuovi ordini finché non termina; gli annullamenti e gli ordini che riducono una posizione partono comunque, e gli ordini attivi non si possono modificare nel frattempo (annullateli).

- **Interruttore di emergenza**: annulla tutti gli ordini attivi su **ogni** conto di trading (inclusi gli ordini inseriti altrove, ogni annullamento nel registro di audit), poi li sospende tutti. In **Impostazioni → Broker** chiede una conferma. Sul grafico si trova nella barra degli strumenti accanto a **Congela**, mentre un conto fa trading, e funziona allo stesso modo: un clic lo arma per 3 secondi, un secondo clic lo fa scattare. Mai una password.
- **Congela** (barra degli strumenti del grafico): blocca ogni nuova entrata inviata dai grafici, su ogni conto collegato (paper incluso), per 10 minuti, e annulla gli ordini di entrata attivi. Chiusure, riduzioni, stop e target continuano a funzionare. Un clic lo arma, un secondo lo avvia; nulla lo termina prima.
- **Limite di perdita**: un conto che raggiunge il proprio limite di perdita si sospende da solo (verificato prima di ogni ordine e nel ciclo di riconciliazione), e ricevete una notifica.
- **Scostamento di posizione**: quando una posizione si muove senza esecuzioni che la spieghino (un ordine che l'app non ha visto, un'esecuzione corretta), il conto si sospende finché non lo **confermate**, e ricevete una notifica. Controllate prima il conto presso il broker; la conferma chiede sempre la vostra password, qualunque sia la politica qui sotto.

In **Impostazioni → Broker → Rischio e interruttore di emergenza** scegliete come termina una sospensione, per tutti i conti:

- **dopo**: l'inizio dell'ora, del giorno o della settimana successiva (lunedì), nel **Fuso orario** della regola; oppure **solo quando revocata**. Il fuso è UTC finché non ne salvate uno, e il modulo parte dal fuso del vostro browser;
- **revoca anticipata con la mia password**: una sospensione può essere revocata prima dalla stessa sezione o dal banner del grafico. Una sospensione che termina solo quando revocata si revoca sempre con la password.

Salvare la regola richiede la vostra password.

Il periodo di perdita segue la stessa scelta (un giorno quando le sospensioni terminano solo se revocate). La perdita si misura dall'equity del conto all'inizio del periodo, come la indica il broker; un deposito o un prelievo nel periodo conta. Un broker che non indica l'equity (Binance Spot Demo Mode) non può essere soggetto a un limite di perdita: i suoi ordini vengono rifiutati finché ne è impostato uno.

## Trading reale {#live-trading}

Il percorso ordini di ogni broker ha un livello, mostrato sul conto in **Impostazioni → Broker**: E2 (testato su una sede simulata), E3 (testato per giorni sull'ambiente di test del broker), E4 (superati una prova a vuoto in reale e un ordine reale della dimensione minima). Sotto E4, un conto che opera con denaro reale viene rifiutato dal connettore stesso.

| Livello | Significa | Ordini reali |
|---|---|---|
| **E2** | testato contro una piattaforma simulata | rifiutati |
| **E2D** | E2, costruito dalla sola documentazione della piattaforma, mai testato contro di essa | su un conto il cui titolare lo ha accettato, con i limiti impostati; il controllo della piattaforma gira prima del primo ordine di ogni sessione dove ne ha uno |
| **E3** | testato nell'ambiente di test del broker, per giorni | rifiutati |
| **E4** | una prova a vuoto reale e un ordine reale della dimensione minima superati | una volta attivato il trading reale, con i limiti impostati |

**E2D (Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex, Deribit).** Attivare il trading reale chiede di spuntare l'accettazione che il percorso degli ordini di questo broker è stato costruito dalla documentazione della piattaforma e non è mai stato testato contro di essa; è salvata con la data e mostrata sul conto. Servono gli stessi limiti di E4. Una risposta che il broker non capisce, un ordine il cui esito resta sconosciuto o una posizione che si muove senza eseguiti sono trattati come su ogni broker: registrati, notificati e riconciliati con il broker.

A E4, un conto reale non accetta ancora alcun ordine finché non attivate **Trading reale** su di esso:

- servono l'interruttore di trading attivo, e il **limite di dimensione o valore per ordine**, il **limite di dimensione o valore per posizione** e il **limite di perdita** del conto impostati;
- il [blocco del trading](#trading-lock) è a vostra scelta. Attivo, l'attivazione del trading reale e il primo ordine di ogni accesso chiedono il codice della vostra app di autenticazione. Disattivato, il conto mostra un promemoria e i suoi ordini partono senza codice;
- il conto mostra il livello, se il broker annulla i vostri ordini quando l'app si ferma (Binance USDⓈ-M: sì; spot e Interactive Brokers: no, gli ordini restano attivi) e quali limiti sono impostati;
- la disattivazione non chiede nulla; anche disattivare il trading la disattiva, e va riattivata.

Gli ambienti di test, i conti demo e i conti paper non ne hanno mai bisogno.

## Registro di audit e metriche degli ordini {#audit-log}

Ogni invio, modifica, annullamento, annullamento globale e chiusura viene scritto in un **registro di audit in sola aggiunta**: ora, conto, broker, azione, richiesta, risposta o errore del broker e latenza. Il database rifiuta di modificare o eliminare una riga. Le chiamate rifiutate prima di raggiungere il broker perché il trading è disattivato o il blocco è chiuso non vengono registrate; ogni altro rifiuto sì. Ciò che la [riconciliazione](#followed-orders) trova (un'esecuzione o un annullamento perso, un ordine esterno, un ordine risolto dopo una risposta persa, una deriva di posizione) viene registrato a sua volta, come **reconcile**, e non è contato nelle metriche degli ordini.

In **Impostazioni → Broker**, ogni conto di trading mostra le sue **metriche degli ordini** (chiamate, inviati, rifiutati, latenza p50 / p99, ultimo ordine) e un pulsante **Annulla tutti gli ordini**. A differenza dell'[interruttore di emergenza](#sospensioni), copre quel solo conto e non sospende nulla:

- chiede conferma, poi annulla **ogni ordine attivo** del conto presso il broker, compresi quelli inseriti fuori da OpenTraderWorld. Da Interactive Brokers sono tutti gli ordini dei conti del Gateway, compresi quelli inviati a mano dalla TWS;
- indica quanti ne ha annullati. Da Interactive Brokers attende prima fino a 10 secondi che non ne resti nessuno attivo;
- funziona con il trading disattivato e con il blocco chiuso.

## Trade su alert {#trade-on-alert}

Un alert del grafico può inviare un ordine quando scatta, senza che nessuno guardi, anche ad app chiusa. Si configura nella sezione **Trade** dell'alert (*Invia un ordine a un conto broker quando questo alert scatta*); l'alert in sé è descritto in [Charts → Alert](/it/modules/market-data#alerts).

- **Il conto** deve avere il trading attivo, essere concesso a Charts e avere il [trading in background](/it/config/brokers#background-trading), firmato una volta in **Impostazioni → Broker**. Un conto reale richiede anche il [trading reale](#live-trading) attivo con i suoi limiti impostati.
- **Quando scatta**: **Entra** invia il ticket che descrivi (lato, tipo, dimensione in **Unità**, **Importo** o **Rischio**, prezzi come distanza dal prezzo che fa scattare l'alert, take profit e stop loss); **Esci** annulla gli ordini in lavorazione sullo strumento e chiude la sua posizione a mercato.
- **Se viene rifiutato**: non fare altro, annulla gli ordini sullo strumento, chiudilo anche, oppure annulla tutto e chiudi ogni posizione. Quando la risposta del broker va persa, vengono annullati solo gli ordini sullo strumento.
- **Anteprima** mostra l'ordine al prezzo del grafico, la simulazione del broker dove ne ha una, e cos'altro è armato sullo strumento. **Arma** viene dopo un'anteprima e chiede il codice del tuo autenticatore quando il [blocco del trading](#trading-lock) è attivo e il conto non è paper.
- **Scatta una volta**, poi aspetta di essere armato di nuovo. Ogni ordine passa ogni controllo che passa un ordine dal grafico ([limiti di rischio](#risk-limits), sospensioni, il registro di audit, con l'alert come fonte), e l'esito è notificato sui canali dell'alert.
- **Disarmo.** Disattivare il trading in background o il trading sul conto, rimuovere il secondo fattore, attivare il blocco del trading, o l'interruttore di emergenza disarma ciò che era armato; armalo di nuovo consapevolmente.

## Limiti

- Da Interactive Brokers, gli ordini vanno al conto predefinito del Gateway.
- Lato e tipo di un ordine attivo non si cambiano: annulli e ne invii uno nuovo.
- Gli ordini sono nella valuta del contratto, senza conversione.
- Gli agenti IA ([MCP](/it/config/ai-agents)) e i workflow dell'Automator non possono inviare ordini, né armare un trade su alert. Un [bot di trading](/it/modules/robots) o un [trade su alert](#trade-on-alert) invia ordini solo dopo che lo hai armato, su un conto con il [trading in background](/it/config/brokers#background-trading) firmato.
- I conti broker, e quindi il trading, sono disattivati in [modalità demo](/it/guide/demo).
