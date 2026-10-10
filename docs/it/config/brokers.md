# Conti broker

Un **conto broker** è una chiave in sola lettura verso il posto in cui fai davvero trading. Risponde a tre domande che nessuno dovrebbe riscrivere a mano: cosa ho eseguito, cosa detengo, cosa ho in lavorazione.

Gestiscili in **Impostazioni → Broker**, oppure dal pulsante *Broker* che ogni modulo che legge il tuo book mette accanto al selettore dell'account. Entrambi mostrano la stessa schermata.

::: warning Sola lettura, per costruzione
I conti broker leggono, e soltanto leggono: nessuna route dietro di essi inserisce, modifica o annulla un ordine. Le credenziali che ti vengono chieste sono del tipo in sola lettura, quindi forniscine esattamente quel tipo: dove un broker può emettere una chiave solo-visualizzazione, il modulo lo dice. Se mai arrivasse l'instradamento degli ordini, sarebbe una funzione a sé, con chiavi e permesso propri, e lo dirà qui.
:::

Da non confondere con i [data connector](/it/config/connectors): quelli leggono i **prezzi**, questi leggono **il tuo account**. Due credenziali diverse, due elenchi diversi, due permessi diversi, di proposito.

## Cosa serve per collegarsi

| Broker | Credenziali | Permesso da concedere | Legge |
|---|---|---|---|
| **Alpaca** | `api_key`, `api_secret` | Chiave Trading API dell'ambiente scelto (live *oppure* paper, sono chiavi separate) | fill, posizioni, ordini, holding |
| **Binance** | `api_key`, `api_secret` | Solo *Enable Reading*. Niente trading, niente prelievi | fill, ordini, holding |
| **Binance USDⓈ-M Futures** | `api_key`, `api_secret` | *Enable Reading* più accesso ai futures. Niente trading, niente prelievi | fill, posizioni, ordini, saldi di margine |
| **Bitget** | `api_key`, `api_secret`, `api_passphrase` | *Read-only*. Niente trade, niente withdraw | fill, posizioni, ordini, holding |
| **OKX** | `api_key`, `api_secret`, `api_passphrase` | Solo *Read*. Niente trade, niente withdraw | fill, posizioni, ordini, holding |
| **OANDA** | `api_token` + l'account id | Un personal access token. **OANDA non ha token in sola lettura**: lo stesso può fare trading | fill, posizioni, ordini, holding |
| **Coinbase Advanced Trade** | `api_private_key` + il nome completo della chiave | Chiave CDP, solo **View**, creata come **Ed25519**. Niente Trade, niente Transfer | fill, ordini, holding |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` + l'account id | Sign-in OAuth che concede `ReadAccount`, `MarketData`, `openid`, `offline_access`. **Non `Trade`** | fill, posizioni, ordini, holding |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | Il tuo login più l'AppKey emessa da StoneX. **Non esiste una credenziale in sola lettura** | fill, posizioni, ordini, holding |
| **Capital.com** | `api_key`, `identifier`, `api_password` | Una chiave API (il secondo fattore deve essere attivo) e la sua password personalizzata. **Nessun grado di sola lettura** | fill, posizioni, ordini, holding |
| **NinjaTrader** | `username`, `password`, `cid`, `sec` | Login della piattaforma più la coppia di chiavi API sviluppatore. **Nessuna chiave in sola lettura** | fill, posizioni, ordini, holding |
| **Kraken** | `api_key`, `api_secret` | *Query Ledger & Trade History* e *Query Open Orders* | fill, ordini, holding |
| **Interactive Brokers (Flex)** | `flex_token` + un id di query Flex | Token del Flex Web Service: legge estratti conto, non può fare trading | fill, posizioni, holding |

I segreti sono in sola scrittura: l'app sa sempre solo *quali* nomi sono impostati. Possono essere digitati, oppure collegati dal [Vault](/it/config/settings#vault) così una chiave serve più account.

### Note per broker

- **Alpaca**: l'impostazione *Environment* è `live` o `paper`, e va dichiarata invece che indovinata, dato che le due vivono su host diversi con chiavi diverse. Alpaca non riporta commissioni su un fill, quindi le operazioni importate non hanno fee: corretto per le azioni senza commissioni, sotto la realtà per crypto e opzioni, le cui fee arrivano come attività di account separate.
- **Binance**, spot e futures, risponde con la sua cronologia di trade **uno strumento alla volta**, quindi un pull deve indicare gli strumenti (`BTCUSDT`, `ETHEUR`). Ogni altro broker qui risponde per l'intero account.
- **Tre venue crypto si fermano a 90 giorni.** Bitget, OKX e Binance futures servono via API tre mesi di fill e non oltre; tutto ciò che è più vecchio è un download dal loro sito. La finestra di import mostra il limite e rifiuta un periodo che inizia prima, invece di restituire una mezza risposta silenziosa.
- **Un account derivati non è uno spot.** Bitget, OKX e Binance futures possono andare short, quindi *Questo account può andare short* è spuntato per impostazione predefinita lì. Su un account Bitget impostato solo sul book spot, deselezionalo: una vendita spot senza nulla di aperto è la vendita di una coin comprata prima, non uno short.
- **Un contratto è contato in contratti.** OKX riporta i fill dei derivati in contratti e pubblica quanto vale uno (`ctVal × ctMult`), che viene letto dalla sua lista di strumenti e riportato come point value del trade. I contratti Bitget USDT-M e Binance USDⓈ-M sono dimensionati nella coin di base, quindi il loro vale uno. I contratti Coin-M (inversi) non sono letti da nessuna parte: sono dimensionati nella valuta di quotazione e il loro PnL non è una quantità per un prezzo.
- **TradeStation è l'unico OAuth.** Non c'è una chiave statica: un sign-in una tantum nel browser produce un refresh token, che l'app scambia con un access token di 20 minuti man mano. Concedi `ReadAccount`, `MarketData`, `openid` e `offline_access` a quel sign-in e lascia fuori `Trade`; un token che potesse fare trading sarebbe un rischio permanente per niente. Un fill qui è una *gamba d'ordine*, dato che TradeStation pubblica ordini chiusi anziché esecuzioni, e l'identità su cui un re-sync deduplica è l'id dell'ordine più la posizione della gamba al suo interno.
- **FOREX.com fa sign-in, non usa una chiave.** Le credenziali sono username e password dell'account più l'AppKey che StoneX emette una volta firmati i termini API, quindi lo stesso login può fare trading: trattalo come un segreto ad accesso completo e cambia la password quando rimuovi l'account. Un trade importato da lì non ha **commissione**, perché StoneX fattura lo spread; se il tuo account paga invece una commissione, i numeri qui saranno sotto la realtà. L'endpoint della cronologia trade non accetta data di fine né cursore, quindi un periodo ampio viene percorso in avanti a pagine da 200.
- **Capital.com risponde un giorno alla volta.** Il suo log di attività limita l'intervallo tra due date a 24 ore, quindi un anno di trading costa una chiamata al giorno; i periodi più ampi di 400 giorni sono rifiutati esplicitamente invece di lasciarli andare a sbattere contro il rate limiter. Ogni strumento della piattaforma è un **CFD**, quindi un CFD su azione viene archiviato come derivato e non come azione, che è ciò che vuole un modulo fiscale. Un deal id indica una posizione e non un fill, quindi l'identità su cui un re-sync deduplica è l'insieme di deal, timestamp e direzione.
- **NinjaTrader consente due sessioni per login**, e una terza chiude la più vecchia: questo connector ne tiene una, e un'applicazione di trading connessa accanto tiene l'altra. La sua API di trading è la piattaforma Tradovate che ha acquisito, ed è per questo che gli errori dicono `tradovateapi`. Risponde con i fill che la sua sessione vede e non pubblica un limite di profondità, quindi controlla la riga più vecchia restituita dal primo pull prima di affidarti a essa per un anno fiscale. Il point value di un future è letto dal prodotto del contratto, mai presunto dalla radice.
- **OANDA non dà un token in sola lettura.** Il personal access token che legge questo account può anche fare trading su di esso, quindi il modulo lo dice: trattalo come una credenziale ad accesso completo e revocalo quando rimuovi l'account. Nulla nell'app lo usa mai per scrivere.
- **Interactive Brokers** non è affatto una chiave API: legge un report salvato tramite il Flex Web Service. Configurazione, regola del periodo e impostazione del fuso orario hanno [una sezione propria più sotto](#interactive-brokers-the-flex-web-service).
- I rate limit sono quelli del broker, e ogni riga mostra quello che si applica. IBKR è il più severo: costruisce l'estratto su richiesta e rifiuta una seconda richiesta finché una è ancora in generazione.

## Collegane uno

1. **Aggiungi account**, scegli il broker e dagli un nome: il nome è ciò che mostrano i selettori dei moduli, quindi *Kraken main* è meglio di *Kraken 2*.
2. Compila le credenziali, oppure scegli dal Vault. Un account a cui ne manca una è mostrato come *incomplete* e saltato da ogni modulo finché non la imposti.
3. Compila le impostazioni non segrete che il broker richiede: un ambiente (Alpaca, Binance futures, TradeStation, Capital.com, NinjaTrader), un account id (OANDA, TradeStation, Capital.com, FOREX.com, NinjaTrader), il nome della chiave Coinbase, l'id di query e l'offset IBKR, o i book Bitget.
4. Scegli i **moduli** che serve, oppure *tutti i moduli*.
5. **Test connection** raggiunge il broker e riporta cosa ha risposto: numero e stato dell'account, quanti asset hanno un saldo, o quale estratto ha restituito il token Flex. Se qualcosa non va, l'errore indica l'impostazione da cambiare.

I permessi sono applicati lato server: un modulo che chiede un account che non gli è mai stato concesso viene rifiutato.

## Cosa ti permette di fare

Quattro destinazioni, una sola forma. Qualunque cosa importi e ovunque finisca, un import da broker corre sugli stessi due binari di un import da file:

1. **Prima preleva, poi guarda.** L'app interroga il broker, integra la risposta in ciò che il modulo salva (posizioni per il journal, un bilancio per il portafoglio, dismissioni chiuse per il modulo fiscale) e te la mostra. In questo passaggio nulla viene scritto, quindi un pull che non ti piace non costa nulla.
2. **Conferma, e tieni il filo.** Tutto ciò che viene scritto porta l'id di quell'import, quindi resta un unico oggetto in seguito: *Annulla* elimina esattamente ciò che ha creato e nient'altro, *Mantieni, non tracciare* taglia il collegamento e lascia le righe al loro posto. Entrambi vivono nella cronologia degli import del modulo.

I duplicati sono compito del binario, non tuo. Ogni riga importata ha un'impronta, quindi prelevare di nuovo un periodo sovrapposto riconosce ciò che è già archiviato, lo segna nell'anteprima e lo scrive una volta sola.

### Importare operazioni nel journal

**Journal → Importa → Preleva da un broker**. Scegli l'account, il periodo e il book in cui archiviare, e i fill tornano integrati in posizioni, in anteprima prima che venga scritto qualcosa.

1. **L'account.** Sono elencati solo quelli concessi al journal, e uno incompleto lo dice. L'ultimo account e periodo usati per un book vengono ricordati, quindi il pull successivo è questione di due clic.
2. **Il periodo**, per data o con i chip *7 / 30 / 90 / 365 giorni*. Leggilo come la finestra in cui cadono i *fill*, non la finestra in cui le operazioni si sono chiuse: una posizione è ricostruita dai fill dentro il periodo, quindi parti abbastanza presto da catturare l'ingresso.
3. **Gli strumenti.** Binance risponde con la sua cronologia uno strumento alla volta, quindi lì i simboli sono obbligatori e *Suggerisci* propone ciò che l'account detiene. Ovunque altrove il campo è un filtro: lascialo vuoto per l'intero account.
4. **Short.** Un account spot non può essere short, quindi una vendita senza nulla di aperto è riportata come errore di riga che indica la soluzione (allarga il periodo) invece di diventare uno short fantasma. Su un account a margine, spunta *Questo account può andare short*.
5. **Anteprima**, poi importa. I contatori sono fill, operazioni, di cui chiuse e aperte, più ciò che è già nel book e ciò che non si è potuto costruire. Ogni riga dice cos'è prima che tu confermi.

- Stessa destinazione e stessa integrazione di un import da file, meno la mappatura: un'API risponde con campi tipizzati, quindi le domande che solleva un CSV (quale colonna è la data, il decimale è una virgola) qui non esistono.
- **Rieseguire è sicuro.** L'identità di una posizione è il suo fill di *apertura*, quindi allargare la finestra e prelevare di nuovo aggiorna ciò che si è chiuso nel frattempo e lascia in pace il resto, invece di archiviare due volte la stessa operazione.
- **Un aggiornamento è solo meccanico.** Prezzi, quantità, fee e date arrivano di nuovo dal broker; le tue note, i tag, la strategia e i campi del modello sono tuoi e sopravvivono.

### Allineare un portafoglio a ciò che detiene l'account

**Portfolio → Da un broker**. Legge un **bilancio**, non una cronologia di operazioni: la differenza rispetto al tuo registro è mostrata riga per riga, e scegli quali righe allineare. Ognuna che accetti scrive la singola operazione che fa concordare il portafoglio.

- Un simbolo che il portafoglio già detiene si risolve da solo; qualsiasi altro viene chiesto, perché "BTC" su un exchange è una stringa e un asset qui è una fonte di prezzo.
- **Il costo base non viene mai inventato.** Interactive Brokers ne pubblica uno e viene usato. Gli exchange crypto pubblicano una quantità e nient'altro, quindi quelle righe lo dicono e usano come predefinito il prezzo di oggi, l'unico prezzo che nessuno può scambiare per un'affermazione sul passato.
- **Prendi il prezzo dall'exchange.** Su una riga senza costo base, un clic chiede alla venue a quanto tratta l'asset in questo momento e compila il prezzo. La riga poi indica il mercato che ha risposto (`BTCUSDT`, `XBT/USD`), e lo dice quando quel mercato quota in qualcosa di diverso dalla valuta propria dell'asset: un prezzo Binance è in USDT, non in dollari. Un asset che la venue non quota viene lasciato stare invece di essere valutato da un'altra parte, e digiti tu il prezzo.

Alpaca, Binance (spot e futures), Bitget, Coinbase, Kraken, OKX, OANDA e TradeStation rispondono a quella domanda. Interactive Brokers Flex no: un estratto non è un feed di quotazioni, FOREX.com quota un mercato con il suo id numerico anziché con il nome che porta una riga di portafoglio, e NinjaTrader serve i prezzi tramite un diritto di dati di mercato separato. Capital.com risponde con il mid dei due lati su cui tratta.

### Disegnare il tuo book sul grafico

**Grafico → Broker book**. Sincronizza un account e le sue posizioni e gli ordini in lavorazione vengono disegnati come livelli di prezzo sul grafico dello strumento corrispondente, costo medio per una posizione, limit e stop per un ordine. L'abbinamento è sul ticker, punteggiatura a parte. Una posizione senza costo medio non ha linea e viene contata come tale.

### Leggere un anno fiscale

**Tasse → Da un broker**. Preleva i fill, li integra in posizioni chiuse e totalizza ciò che è stato realizzato nell'anno fiscale, ripartito tra le righe capitale, derivati e crypto del modulo. **Non scrive nulla**: applichi tu i numeri al modulo e salvi tu lo scenario.

- **La finestra non è l'anno fiscale.** Ciò che hai venduto a marzo è stato comprato prima, e senza quell'acquisto non c'è costo base: porta indietro la data di inizio abbastanza da coprirlo. Una dismissione di cui manca l'acquisto viene segnalata, mai valutata contro il nulla.
- Ogni posizione chiusa è convertita al cambio della **propria data di uscita**. Una senza cambio viene elencata e lasciata fuori dai totali invece di essere sommata nella valuta sbagliata.

## Interactive Brokers: il Flex Web Service {#interactive-brokers-the-flex-web-service}

IBKR è l'unico broker qui che non si legge tramite un'API di trading. Si legge tramite **Flex**, il servizio di report di Account Management: salvi una query che descrive cosa vuoi in un estratto, e l'app preleva quell'estratto via HTTPS con un token.

### Perché Flex e non TWS

Il [connector di dati di mercato](/it/config/connectors#interactive-brokers) parla il socket TWS con un Gateway che esegui tu. Quel socket è lo strumento giusto per il presente e quello sbagliato per lo storico: risponde con le posizioni aperte e i fill della **sessione corrente**, quindi *importa le mie operazioni di marzo* non ha alcuna forma via socket. Flex serve un periodo, che è esattamente la domanda che pone un import.

La differenza pratica:

| | Flex Web Service | Socket TWS / IB Gateway |
|---|---|---|
| **Cosa esegui** | niente, è una chiamata HTTPS | Gateway o TWS, connesso, sulla macchina |
| **Storico** | il periodo della query, fino a un anno indietro | solo la sessione corrente |
| **Credenziale** | un token che legge report | la tua sessione live, in grado di fare trading |
| **Usato qui per** | import nel journal, holding del portafoglio, anno fiscale, posizioni sul grafico | prezzi, grafici, barre live |

### Perché è la via d'ingresso sicura

- **Il token non può fare trading.** È emesso per il Flex Web Service e quel servizio serve estratti. Non c'è alcun endpoint di ordini dietro da dimenticare di disabilitare, nessuna casella di permesso da sbagliare. Confrontalo con una chiave API di un exchange, dove sola lettura è una casella che devi ricordarti di spuntare.
- **Nulla resta in ascolto.** Nessun Gateway in esecuzione, nessuna porta API aperta, nessun Trusted IP da dichiarare, nulla in attesa sulla tua macchina mentre l'app è inattiva.
- **Scade da solo.** IBKR dà una durata al token e invia un promemoria via mail prima che scada. Un token dimenticato smette di funzionare invece di restare valido per sempre.
- **La query è la recinzione.** Un token può restituire solo ciò che descrivono le query che hai salvato. Tieni la query a operazioni e posizioni aperte e questo è tutto ciò che l'app potrà mai vedere, qualunque cosa chieda.
- Come ogni credenziale qui, il token è **in sola scrittura nell'app**: si può digitare o collegare dal [Vault](/it/config/settings#vault), e non viene più mostrato.

### Come funziona davvero un pull

1. L'app chiama `SendRequest` con il tuo token e l'id della query. IBKR risponde con un codice di riferimento e inizia a **costruire** l'estratto.
2. Poi interroga `GetStatement` con quel codice finché arriva l'XML, normalmente qualche secondo e può essere di più per una query ampia. Un estratto ancora in generazione è la risposta attesa ai primi tentativi, non un errore.
3. L'estratto viene analizzato in fill (righe `Trade` a livello di esecuzione) e holding (`Open Positions`), e l'app li filtra sul periodo che hai scelto.

Se IBKR sta ancora generando dopo un minuto l'app lo dice invece di restare appesa: restringi l'intervallo di date della query, oppure riprova tra un momento.

### Configurazione

1. **Il token**: Account Management → Settings → **Flex Web Service**. Generane uno, copialo una volta, annota la data di scadenza.
2. **La query**: Account Management → Performance & Reports → **Flex Queries** → nuova query *Activity*. Includi:
   - **Trades**, livello di dettaglio **Execution**, per il journal e il modulo fiscale;
   - **Open Positions**, per l'allineamento del portafoglio e la sovrapposizione sul grafico.

   Salvala e annota l'**id della query**, il numero mostrato accanto al suo nome.
3. In OpenTraderWorld: aggiungi l'account, incolla il token, compila **Flex query id** e **Statement time offset**, poi **Test connection**. Riporta il numero di account, il periodo che l'estratto copre e quante righe di trade contiene, il modo più rapido per vedere che alla query manca una sezione.

::: tip Due impostazioni che decidono se l'import è giusto
**Il periodo è quello della query, non il tuo.** Una query Flex porta il proprio intervallo di date (*Last 365 Calendar Days*, *Year to Date*, una finestra personalizzata) e il web service non accetta date. Le date che scegli nell'app **filtrano** ciò che l'estratto ha restituito, quindi una query impostata su *Last 30 days* non darà mai marzo per quanto indietro tu chieda, e IBKR non serve più di un anno. Imposta la query ampia, filtra nell'app.

**Un estratto Flex non nomina mai il suo fuso orario.** Marca il fuso proprio della query e non dice quale, quindi imposta *Statement time offset* su quel fuso in minuti (`-300` New York in inverno, `60` Parigi) o ogni fill finisce all'ora sbagliata, e le operazioni intraday finiscono nel giorno sbagliato.
:::

### Cosa Flex non fa

- **Nessun ordine in lavorazione**, quindi la sovrapposizione sul grafico disegna le posizioni IBKR al loro costo medio e nessun livello di ordine.
- **Nessuna quotazione.** Un estratto non è un feed di prezzi: il pulsante *prendi il prezzo dall'exchange* del portafoglio è offerto dai broker con API, non da qui. IBKR è l'unico dei cinque che pubblica un **costo base**, il numero che conta per un registro.
- **Un estratto alla volta.** IBKR lo costruisce su richiesta e rifiuta una seconda richiesta mentre una è in generazione, quindi pull consecutivi sulla stessa query si aspettano a vicenda.

## Permessi per modulo

| Modulo | Cosa legge |
|---|---|
| **Trading Journal** | i fill del periodo, per l'import |
| **Portfolios** | ciò che l'account detiene |
| **Visualization** | posizioni e ordini in lavorazione, per la sovrapposizione sul grafico |
| **Tax Calculator** | i fill di un anno fiscale |

Spuntare ogni modulo torna a collassare nel wildcard *tutti i moduli*, che copre anche i moduli aggiunti nelle versioni future.

## Limiti

- **Un ticker non viene mai indovinato.** Un simbolo che l'app non riesce a risolvere è un errore che indica la soluzione, non una corrispondenza approssimativa.
- Ciò che un broker non pubblica resta vuoto invece che plausibile: nessun costo base inventato, nessuna fee inventata, nessun lato inventato.
- Eliminare un account rimuove le sue credenziali. Ciò che ha già importato resta.
- I conti broker sono disabilitati in [modalità demo](/it/guide/demo).
