# Fare trading dal grafico

Un conto broker può fare più che leggere il suo book: con il **trading** attivato, il grafico gli invia ordini, li sposta e li annulla dalla loro linea, e chiude le posizioni dalla loro. Oggi accettano ordini tre broker:

- **Interactive Brokers (Gateway)**, tramite l'IB Gateway o la TWS che esegue lei stesso. Questa pagina.
- **Binance** e **Binance USDⓈ-M Futures**, sui conti reali e sui loro ambienti di test (Spot Demo Mode, il testnet dei futures). Un conto reale accetta ordini una volta attivato il [trading reale](#live-trading) su di esso.
- **Paper trading**, un conto simulato gestito dall'applicazione stessa. Vedere [Paper trading](/it/config/paper-trading).

Ogni altro broker risponde che non accetta ordini da OpenTraderWorld e resta in [sola lettura](/it/config/brokers).

::: danger Sperimentale
Il trading dal grafico è **sperimentale**: per ora funziona con Interactive Brokers, Binance e Binance USDⓈ-M Futures. È ampiamente testato, su mercati simulati e sugli ambienti di test dei broker, ma possono restare dei bug. Controlli ogni ordine presso il broker, inizi in piccolo e faccia trading a suo rischio.
:::

::: tip Conti reali
Interactive Brokers, Binance e Binance USDⓈ-M Futures sono classificati **E4**: un conto reale accetta ordini una volta attivato il [trading reale](#live-trading) su di esso, con i suoi limiti impostati. I conti paper e gli ambienti di test non ne hanno bisogno.

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

| | Interactive Brokers | Binance spot | Binance USDⓈ-M | Paper |
|---|---|---|---|---|
| **Tipi** | tutti | tutti | tutti | tutti |
| **Validità** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK |
| **Uscite, OCO** | collegate da IB | liste di ordini (OTO, OTOCO, OCO) | collegate dall'applicazione | collegate dall'applicazione |
| **Trailing stop** | importo o %, dal mercato | %, a passi di 0,01 %, prezzo di partenza | % (da 0,1 a 20), prezzo di partenza | importo o %, prezzo di partenza |
| **Reduce only** | | | sì | sì |
| **Post only** | | sì (limit maker) | sì (GTX) | sì |
| **Iceberg** | sì, unità intere | sì, limit GTC | | |
| **Trigger on** | ultimo, bid/ask, mid | | ultimo, mark | |
| **Close position** | | | sì | |
| **Outside RTH** | sì | | | |
| **Adaptive algo** | sì | | | |

- **Binance spot** trattiene le uscite di un ingresso **limit** (OTO, OTOCO) e piazza quelle di un ingresso **market** una volta eseguito; un ingresso stop non accetta uscite. La metà limit di un OCO parte come limit maker. Un'uscita di vendita è per la quantità acquistata: una commissione prelevata nella moneta stessa ne lascia meno, e Binance rifiuta l'uscita.
- **Binance USDⓈ-M** non collega nulla da sé: l'applicazione invia le uscite come ordini reduce-only accanto al loro ingresso, e le collega, insieme alle coppie OCO, leggendole ogni 5 secondi. Dopo un riavvio l'applicazione riprende i collegamenti dagli ordini che ha salvato. L'hedge mode è seguito: ogni ordine indica il suo lato long o short, e un simbolo detenuto in entrambe le direzioni si chiude un lato alla volta.
- I trailing stop di **Interactive Brokers** seguono dal momento in cui vengono piazzati (nessun prezzo di partenza) e mostrano un iceberg in unità intere.

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
| **Limite di perdita per periodo (valuta del conto)** | qualsiasi ordine una volta che il conto ha perso questo importo dall'inizio del periodo (ora, giorno o settimana, vedi sotto): il conto viene sospeso |
| **Finestra dei duplicati (secondi)** | lo stesso ordine (lato, tipo, dimensione, prezzi) inviato di nuovo entro questi secondi, finché non confermate |

- Una violazione viene rifiutata indicando limite e valore. **L'ordine non viene mai ridimensionato per rientrare.**
- **Ridurre il rischio passa sempre**: un ordine reduce-only, una chiusura o un ordine che riduce soltanto ciò che è detenuto supera ogni limite e ogni sospensione.
- Modificare un limite richiede la vostra password.

### Sospensioni

Una sospensione rifiuta i nuovi ordini finché non termina; gli annullamenti e gli ordini che riducono una posizione partono comunque, e gli ordini attivi non si possono modificare nel frattempo (annullateli).

- **Interruttore di emergenza**: il pulsante in **Impostazioni → Broker** e nel pannello ordini del grafico annulla tutti gli ordini attivi su **ogni** conto di trading (inclusi gli ordini inseriti altrove, ogni annullamento nel registro di audit), poi li sospende tutti. Chiede una conferma, mai una password.
- **Limite di perdita**: un conto che raggiunge il proprio limite di perdita si sospende da solo (verificato prima di ogni ordine e nel ciclo di riconciliazione), e ricevete una notifica.
- **Scostamento di posizione**: quando una posizione si muove senza esecuzioni che la spieghino (un ordine che l'app non ha visto, un'esecuzione corretta), il conto si sospende finché non lo **confermate**, e ricevete una notifica. Controllate prima il conto presso il broker; la conferma chiede sempre la vostra password, qualunque sia la politica qui sotto.

In **Impostazioni → Broker → Rischio e interruttore di emergenza** scegliete come termina una sospensione, per tutti i conti:

- **dopo**: l'inizio dell'ora, del giorno o della settimana successiva (lunedì), nel **Fuso orario** della regola; oppure **solo quando revocata**. Il fuso è UTC finché non ne salvate uno, e il modulo parte dal fuso del vostro browser;
- **revoca anticipata con la mia password**: una sospensione può essere revocata prima dalla stessa sezione o dal banner del grafico. Una sospensione che termina solo quando revocata si revoca sempre con la password.

Salvare la regola richiede la vostra password.

Il periodo di perdita segue la stessa scelta (un giorno quando le sospensioni terminano solo se revocate). La perdita si misura dall'equity del conto all'inizio del periodo, come la indica il broker; un deposito o un prelievo nel periodo conta. Un broker che non indica l'equity (Binance Spot Demo Mode) non può essere soggetto a un limite di perdita: i suoi ordini vengono rifiutati finché ne è impostato uno.

## Trading reale {#live-trading}

Il percorso ordini di ogni broker ha un livello, mostrato sul conto in **Impostazioni → Broker**: E2 (testato su una sede simulata), E3 (testato per giorni sull'ambiente di test del broker), E4 (superati una prova a vuoto in reale e un ordine reale della dimensione minima). Sotto E4, un conto che opera con denaro reale viene rifiutato dal connettore stesso.

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

## Limiti

- Da Interactive Brokers, gli ordini vanno al conto predefinito del Gateway.
- Lato e tipo di un ordine attivo non si cambiano: annulli e ne invii uno nuovo.
- Gli ordini sono nella valuta del contratto, senza conversione.
- Gli agenti IA ([MCP](/it/config/ai-agents)) e i workflow dell'Automator non possono inviare ordini.
- I conti broker, e quindi il trading, sono disattivati in [modalità demo](/it/guide/demo).
