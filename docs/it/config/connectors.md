# Data connector

Ogni modulo che legge dati di mercato attinge da **un unico elenco condiviso di connector**. Un account provider si crea una volta e si concede ai moduli che possono usarlo.

Gestiscili in **Impostazioni → Data connector**, nella pagina autonoma **/connectors**, o dal pulsante del connector che ogni modulo dati mette accanto al selettore del provider. Tutti e tre mostrano la stessa schermata.

## Cos'è un connector

Un **connector è un'istanza con nome di un provider**, non il provider stesso. Gli appartengono quattro cose:

- **il provider**: Binance, Yahoo Finance, EODHD…;
- **le sue credenziali**: digitate, o collegate dal [Vault](/it/config/settings#vault). In sola scrittura: l'app sa sempre solo *quali* nomi di segreto sono impostati;
- **un limite di richieste opzionale**: un numero massimo di chiamate per periodo;
- **i moduli autorizzati a usarlo**: uno o più, oppure *tutti i moduli* (un wildcard che copre anche i moduli dati aggiunti nelle versioni future).

Più connector dello stesso provider possono coesistere. È questo il punto: una chiave in sola lettura per i grafici e una chiave separata per i download in blocco, ciascuna con il proprio limite, ciascuna concessa a un modulo diverso.

## Provider

| Provider | Credenziali | Tipi di asset | Ricerca simboli | Stream live |
|---|---|---|---|---|
| **Binance** | nessuna | crypto | sì | sì |
| **Binance USDⓈ-M Futures** | nessuna | crypto | sì | sì |
| **Bitget** | nessuna | crypto | sì | sì |
| **OKX** | nessuna | crypto | sì | sì |
| **Kraken** | nessuna | crypto | sì | sì |
| **Coinbase** | nessuna | crypto | sì | sì |
| **OANDA** | `api_token` (+ account id) | FX e CFD | sì | no |
| **Yahoo Finance** | nessuna | azioni, ETF, indici, crypto | sì | no |
| **Alpha Vantage** | `api_key` | azioni, ETF, crypto, FX | sì | no |
| **EODHD** | `api_key` | azioni, ETF, FX, crypto | sì | no |
| **Alpaca** | `api_key`, `api_secret` | azioni, crypto, opzioni | sì | intraday |
| **Massive (Polygon.io)** | `api_key` | azioni, ETF, opzioni, future, crypto, FX, indici | sì | intraday, piano a pagamento |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` | azioni, ETF, opzioni, future, indici | per simbolo | no |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | FX e CFD | sì | no |
| **Capital.com** | `api_key`, `identifier`, `api_password` | FX, indici, azioni, crypto (tutti CFD) | sì | sì |
| **Interactive Brokers** | nessuna (host + porta) | azioni, ETF, crypto, FX, indici, future, opzioni | sì | intraday |

Quelli senza chiave funzionano nel momento in cui crei il connector. Ogni riga di provider rimanda alla propria documentazione API e riporta una nota sui rate limit, e un connector che fa streaming riporta anche una nota su cosa costa il live.

### Streaming live {#live-streaming}

La copertura live è più ristretta di quella dei download, e di proposito.

- Gli exchange crypto pubblicano un canale di candele per intervallo, quindi **ogni** timeframe che scaricano lo trasmettono anche in streaming, compreso il giornaliero: su un mercato 24/7 la candela giornaliera *è* il giorno epoch. Bitget e OKX ancorano le proprie candele giornaliere e settimanali alla mezzanotte di Hong Kong, quindi sia il download sia il feed live chiedono invece le varianti allineate a UTC, e una serie scaricata lì si allinea con una scaricata ovunque altrove.
- **Tre provider qui non fanno streaming.** OANDA e TradeStation pubblicano i prezzi live su una risposta HTTP di lunga durata, e FOREX.com su Lightstreamer; nessuno dei tre è il WebSocket che i grafici live parlano. I loro download storici e il lato account funzionano; la candela live no.
- Alpaca, Massive e Interactive Brokers pubblicano ciascuno una sola granularità (rispettivamente barre da un minuto, aggregati da un minuto e barre da cinque secondi) e il timeframe del grafico viene ricavato da essa. Questo rende disponibile ogni timeframe **intraday** e lascia **giornaliero e settimanale al download**: una sessione azionaria non è fatta di 1440 minuti allineati all'epoch, quindi una candela giornaliera costruita così non coinciderebbe con quella salvata dal download. Il grafico lo dice invece di nascondere il controllo.
- Il live viene spesso venduto separatamente dallo storico. Una chiave Massive gratuita scarica lo storico e viene rifiutata al login live; la chiave gratuita di Alpaca trasmette IEX e il feed indicativo delle opzioni ma non SIP o OPRA; Interactive Brokers serve ciò a cui il tuo account è abbonato. Quando un feed non può partire, il grafico dice quale di questi casi è e si ferma, invece di riconnettersi dietro un pallino che non diventa mai verde.
- La maggior parte di questi vendor consente **una connessione live per account**, quindi un secondo programma sulla stessa chiave prende il posto. Quel caso viene segnalato come tale e continua a riprovare, dato che si risolve quando chiudi l'altro.

**Alpaca** ha una sola impostazione per questo: *Market data feed*, `iex` (piano gratuito, il predefinito) o `sip` (a pagamento). Seleziona solo il socket live; i download non sono influenzati.

### I provider di derivati crypto

`BTCUSDT` è una coppia spot **e** un perpetual, e le due sono serie diverse: il perpetual tratta con un basis rispetto allo spot e un contratto con scadenza converge verso di esso. Quindi quale mercato contenga un dataset non viene mai dedotto dal ticker.

- **Binance USDⓈ-M Futures** è un provider a sé accanto a Binance, non un'impostazione di quest'ultimo. I contratti sono scritti come li scrive il mercato dei futures: `BTCUSDT` per un perpetual, `ETHUSDT_250926` per uno con scadenza. I contratti Coin-M (inversi) non sono serviti.
- **Bitget** ha un'impostazione *Market*, `spot` (il predefinito) o `usdt-futures`, perché scrive lo stesso ticker in modo identico su entrambi i book.
- **OKX** non richiede impostazioni: i suoi id di strumento dicono su quale mercato è un ticker, `BTC-USDT` per lo spot, `BTC-USDT-SWAP` per un perpetual, `BTC-USD-241227` per un contratto con scadenza.

### OANDA

L'unico provider FX con chiave qui, e la sua chiave è quella dell'account: OANDA non emette token solo per dati di mercato, quindi il connector chiede lo stesso personal access token usato dal [conto broker](/it/config/brokers), più il numero di account tramite cui legge i prezzi.

- **Impostazioni**: *Account ID* (`001-004-1234567-001`) ed *Environment* (`live` o `practice`, che sono host diversi con token diversi).
- Gli **strumenti** si scrivono `base_quote`, indici e materie prime compresi: `EUR_USD`, `XAU_USD`, `SPX500_USD`. *Test connection* riporta quanti l'account è autorizzato a quotare.
- **Le candele giornaliere sono ancorate alla mezzanotte UTC.** Il predefinito di OANDA fa cambiare giorno alle 17:00 di New York, che è la sessione FX ma non il giorno su cui è salvato ogni altro dataset qui, quindi il connector chiede quello UTC.
- Il volume è un **conteggio di tick**, non una dimensione scambiata: un dealing desk pubblica quanti prezzi ha fatto, non quanto è passato di mano.

Quando un provider ha più connector, il controllo live del grafico acquisisce un selettore di account: due chiavi sono due diritti e due posti di connessione, quindi quale si consuma è una tua scelta, non un ripiego.

### TradeStation

Il suo scope di dati di mercato viaggia sulla chiave OAuth dell'account, quindi il connector chiede la stessa coppia di chiavi API e lo stesso refresh token usati dal [conto broker](/it/config/brokers). Non esiste una credenziale separata per i dati di mercato.

- **Impostazioni**: *Environment* (`live` o `sim`).
- I **simboli** sono quelli di TradeStation: `AAPL` per un'azione, `@ES` per il future continuo, `ESH26` per un contratto, `$SPX.X` per un indice cash, `MSFT 260116C400` per un'opzione. Digitarne uno lo cerca e mostra cos'è, il modo più rapido per scovare un refuso.
- **Le barre sono marcate alla chiusura**, quindi il connector sottrae l'intervallo e salva l'apertura, come ogni altra serie qui.
- **Sono offerti solo 1m, 5m, 15m e 1d.** TradeStation costruisce le barre intraday dall'apertura della *sessione*, quindi la sua candela oraria parte alle 9:30 e non si allineerebbe con la candela oraria di nessun altro posto della tua libreria. Scarica 15m e leggilo a qualsiasi timeframe intraday; il rifiuto lo dice.

### FOREX.com (StoneX)

Stessa storia: nessuna credenziale di dati di mercato propria, quindi accede con lo stesso username, password e AppKey del [conto broker](/it/config/brokers), e i due condividono una sessione.

- **Un mercato è un numero.** L'API prende un id di mercato numerico; tu digiti `EUR/USD` e il connector lo risolve. Quando un nome corrisponde a più mercati l'errore li elenca con i loro id, e scarichi per id.
- **Nessun volume.** Un dealing desk pubblica prezzi, non dimensioni, quindi la colonna del volume è zero invece di un numero plausibile.
- **Una barra giornaliera è la sessione del venue**, che cambia alla chiusura di New York, non a mezzanotte UTC. È il periodo in cui StoneX ha effettivamente scambiato ed è salvato come tale, quindi una serie giornaliera di qui non si sovrappone a una di un provider a giorno UTC.
- `4h` è rifiutato: StoneX non dice da dove inizia a contarlo. Scarica `1h` e leggilo a 4h.

### Capital.com

Il terzo con chiave che è quella dell'account: Capital.com non emette credenziali per dati di mercato, quindi il connector accede con la stessa chiave API, login e password personalizzata del [conto broker](/it/config/brokers), e i due condividono una sessione.

- **Impostazioni**: *Environment* (`live` o `demo`).
- **Uno strumento è un epic**, il nome di mercato di Capital.com: `EURUSD`, `US500`, `AAPL`, `BTCUSD`. La ricerca simboli li restituisce.
- **Una candela è il mid dei due lati** su cui tratta il desk, sia nel download sia sul grafico live.
- **Una barra giornaliera è la sessione del venue**, non il giorno UTC, quindi una serie giornaliera di qui non si sovrappone a una di un provider a giorno UTC.
- Il **live** viaggia sulla stessa sessione e consente 40 strumenti alla volta. Capital.com trasmette bid e ask come due candele separate, quindi un riquadro si riempie quando entrambi i lati hanno fatto tick.

### Interactive Brokers {#interactive-brokers}

Il caso a parte: non c'è URL del vendor né chiave API. Esegui **IB Gateway** o **TWS** sulla tua macchina e il connector parla il suo protocollo socket, quindi ciò che porta è un **indirizzo**, non una credenziale: un host e una porta, salvati in chiaro così una connessione fallita si può diagnosticare. I dati sono quelli a cui è abbonato il tuo account IB.

- **Impostazioni**: *Gateway host* (`host.docker.internal` per un gateway sulla stessa macchina, dato che OpenTraderWorld gira in un container) e *API port* (4001 live / 4002 paper per il Gateway, 7496 / 7497 per TWS).
- **Nel gateway**: Global Configuration → API → Settings, spunta *Enable ActiveX and Socket Clients*, e verifica che la porta corrisponda. Su Docker Desktop la chiamata arriva dal loopback dell'host, quindi *Allow connections from localhost only* la copre già; su Docker Engine deseleziona l'opzione e aggiungi `172.28.53.10` a *Trusted IPs*, che accetta indirizzi singoli e non un intervallo.
- **Test connection** riporta cosa ha risposto, e indica l'impostazione da cambiare quando nulla risponde.
- **Ticker**: `AAPL`, `SAN:EUR` o `7203@TSEJ:JPY` per le azioni, `EURUSD` per una coppia cash, un simbolo OCC per un'opzione. Un future si scrive con il suo mese, `ES.202512`, o con il simbolo locale che mostra TWS, `MNQU6`. I future vengono cercati sul gateway prima di scaricare qualsiasi cosa, quindi l'exchange è opzionale: quando il ticker indica più di una quotazione, l'errore le elenca e tu scegli.
- Il client id è scelto dall'app in una fascia privata alta, mai richiesto, quindi nulla di ciò che hai collegato al gateway viene disconnesso.
- Interactive Brokers consente 60 richieste storiche per finestra mobile di 10 minuti **per account**: il connector si dà il ritmo da solo, quindi un backfill lungo è lento per progetto.

Testato con **IB Gateway build 10.50.1e (25 ago 2026)**. Le build più vecchie dovrebbero funzionare, dato che il protocollo socket viene negoziato al ribasso, ma quella è la versione su cui questo connector è stato verificato.

## Creane uno

1. **Aggiungi connector**, scegli il provider e dagli un nome: il nome è ciò che mostrano i selettori dei moduli, quindi *Binance charts* è meglio di *Binance 2*.
2. Compila le credenziali richieste dal provider, oppure scegli dal Vault. I provider senza chiave saltano questo passaggio.
3. Scegli i **moduli** che serve. Aperto da un modulo, il nuovo connector viene concesso solo a quel modulo; aperto dalle Impostazioni o da `/connectors`, viene concesso a tutti.
4. Facoltativamente imposta un **limite di richieste** (vedi sotto).

Un connector a cui manca una credenziale richiesta è mostrato come *needs credentials* ed è saltato da ogni modulo finché non la imposti.

## Permessi per modulo

L'elenco dei permessi è **lato server**: un modulo che chiede un connector che non gli è mai stato concesso viene rifiutato, quindi una casella che vivesse solo nel browser sarebbe decorazione. Spuntare ogni modulo dati torna a collassare nel wildcard *tutti i moduli*, che mantiene coperti i futuri moduli dati.

I moduli concedibili oggi:

| Modulo | Cosa legge |
|---|---|
| **Historical Data** | l'elenco dei provider del modulo di download e la ricerca dei simboli |
| **Visualization** | la ricerca simboli del grafico, le sue finestre su richiesta e il suo stream live |
| **Watchlists** | la fonte delle quotazioni di una lista, o di un singolo simbolo |
| **Journal** | le candele dietro le schede Market data e Open risk |
| **Quant Tools** | le schede Derivatives: contratti future, catene di opzioni e volatilità implicita, da Interactive Brokers o Massive |

## Limiti di richieste {#request-limits}

Un limite è un conteggio di chiamate in uscita per **giorno**, **ora** o **minuto**, tracciato per connector.

- Ovunque altrove nell'app è **solo osservazione**: alimenta i contatori in [Impostazioni → Frequenza API](/it/config/settings#api-rate) e ti avvisa, ma nulla viene limitato.
- Sui fetch su richiesta del grafico (`/api/histviz/series`) **blocca**: quando il connector raggiunge il limite la finestra torna con le barre già salvate e un avviso *request limit reached*, invece di bruciare in silenzio un piano a consumo.

Lascia il limite disattivato se preferisci che sia il provider a dire no.

## Dove si usano i connector

- **Historical Data**: l'elenco dei provider del modulo di download e la ricerca dei simboli.
- **Visualization**: la scheda Data cerca in tutti i connector concessi al grafico insieme; lo stream live gira sul connector che scegli nel controllo live, o sul più vecchio concesso al grafico per quel provider.
- **Watchlists**: la fonte delle quotazioni di una lista, o di un singolo simbolo. CoinGecko e Yahoo restano disponibili senza alcun connector.
- **Trading Journal**: le candele dietro le schede Market data e Open risk, con una fonte selezionabile per tipo di asset.
- **Quant Tools**: le schede Derivatives elencano i contratti future di un prodotto o la catena di opzioni di un sottostante e prezzano ciascuno. Solo Interactive Brokers e Massive li elencano; la storia della volatilità implicita viene dal solo Interactive Brokers.

::: tip Aggiornamento dalle vecchie impostazioni per modulo
La scheda *Settings* di Historical Data e la scheda *Sources* di Watchlists non esistono più: erano due copie scollegate di questa schermata su due elenchi scollegati. Gli account creati in uno dei due ora sono connector qui, ciascuno ancora concesso al modulo da cui veniva, quindi all'aggiornamento la copertura non cambia. I nomi sono di nuovo univoci globalmente: un nome presente in entrambi gli elenchi viene tenuto una volta e l'altro rinominato `<name> #2`.
:::
