# Paper trading

Un **conto di paper trading** è un broker gestito da OpenTraderWorld stesso. Gli ordini che riceve vengono eseguiti al prezzo in tempo reale del grafico da cui partono, contro la sua liquidità simulata, con le commissioni, lo slippage e la leva che imposta lei. Nessuna chiave, nessun mercato, nessun denaro reale.

Si opera dal grafico esattamente come con un broker reale: lo stesso ticket, la stessa barra rapida, le stesse linee di ordini e posizioni, schede del pannello e registro di audit. Sono descritti in [Fare trading dal grafico](/it/config/trading); questa pagina tratta ciò che è proprio della simulazione.

## Crearne uno

**Impostazioni → Broker → Aggiungi conto**, broker **Paper trading**. Nessuna credenziale. Le impostazioni (etichettate in inglese dall'applicazione):

| Impostazione | Predefinito | Significato |
|---|---|---|
| **Account currency** | `USD` | La valuta di liquidità e risultati, da 3 a 5 lettere |
| **Starting balance** | `100000` | Registrato come primo deposito |
| **Commission per order** | 0 | Un importo fisso per esecuzione |
| **Commission per unit** | 0 | Per azione o contratto |
| **Commission (% of value)** | 0 | `0.1` significa 0,1 % del valore negoziato |
| **Minimum commission** | 0 | Un'esecuzione non costa mai meno |
| **Slippage (basis points)** | 0 | `5` significa 0,05 %, a suo sfavore |
| **Leverage** | 1 | 1 è un conto in contanti |
| **Short selling** | yes | `no` rifiuta una vendita senza nulla da vendere |
| **Fill on** | `quote` | `quote` esegue a bid e ask, `last` all'ultimo prezzo |

I numeri devono essere zero o più, e la leva almeno 1. Le impostazioni sono rilette a ogni esecuzione: una modifica vale dalla successiva.

Un nuovo conto paper ha attivi **Tutti i moduli** e **Trading**. Il trading su un conto paper non chiede password e non è mai dietro il [blocco del trading](/it/config/trading#trading-lock).

**Prova connessione** risponde con equity, liquidità, posizioni aperte e ordini attivi del conto.

## Liquidità

Il pannello **Conto paper**, sotto il conto in Impostazioni → Broker, ha un campo **Importo** e tre pulsanti:

- **Deposita** aggiunge l'importo alla liquidità.
- **Preleva** lo toglie, fino alla liquidità detenuta (non l'equity).
- **Reimposta conto** elimina ogni ordine, esecuzione e movimento di liquidità, poi riparte da un unico deposito: l'importo digitato, o il saldo iniziale se il campo è vuoto. Le impostazioni restano, e i trade già importati nel journal rimangono.

Il pannello mostra anche il totale depositato e gli ultimi 50 ordini non più attivi con stato (eseguito, annullato, rifiutato, scaduto) e motivo. **È in questo elenco che un rifiuto viene spiegato.**

## Come vengono eseguiti gli ordini

### A quale prezzo

Un ordine viene eseguito sul **flusso in tempo reale del connettore dati del grafico**: il connettore da cui è partito l'ordine, o il connettore del fornitore concesso ai Grafici. Il motore gira sul server, quindi **gli ordini continuano a essere eseguiti a grafico chiuso**, e si fermano mentre l'applicazione è spenta.

- Uno strumento senza flusso in tempo reale viene rifiutato all'invio. Anche un indice: non c'è un mercato da negoziare.
- Il motore segue le barre in tempo reale più fini del fornitore (1 minuto se disponibile) e le sue quotazioni quando il flusso le pubblica. Contano solo i prezzi stampati dopo che l'ordine è diventato attivo.
- Con **Fill on: quote**, gli acquisti si eseguono all'ask e le vendite al bid, finché la quotazione ha meno di 15 secondi. Altrimenti, e con **last**, entrambi all'ultimo prezzo.
- Una barra più vecchia di 2 minuti non viene usata. Un ordine a mercato inviato a mercato chiuso attende il primo prezzo fresco invece di essere rifiutato.
- Se il flusso si ferma, viene ritentato ogni 10 secondi e gli ordini attendono. Se il connettore dati viene eliminato, i suoi ordini non possono più essere eseguiti.

### Per tipo di ordine

| Tipo | Eseguito quando | A |
|---|---|---|
| **Market** | al prezzo successivo | quel prezzo, più slippage |
| **Limit** | il mercato raggiunge il limite | il limite, o meglio se il mercato lo ha già superato. Senza slippage |
| **Stop** | il mercato raggiunge lo stop | lo stop se toccato, il mercato in caso di gap, più slippage |
| **Stop limit** | si raggiunge lo stop, poi il limite | come un limit, eventualmente allo stesso prezzo |
| **Trailing stop** | il mercato ritorna della distanza di inseguimento dal suo prezzo migliore dall'inizio dello stop (il più alto dietro una vendita, il più basso dietro un acquisto) | come uno stop |
| **MIT** | il mercato arriva al trigger dal lato del limit | il mercato, più slippage |
| **LIT** | il trigger, poi il limite | come un limit |

- **Nessuna esecuzione parziale**: un ordine viene eseguito per intero, senza controllo di volume o liquidità.
- Un **trailing stop** con prezzo di partenza attende che il mercato lo raggiunga (salire fino a esso dietro una vendita, scendere fino a esso dietro un acquisto), poi segue da quel prezzo. Il suo prezzo migliore viene conservato attraverso un riavvio.
- **Reduce only**: piazzato da solo, viene rifiutato a meno che il conto non detenga una posizione sul lato opposto. All'esecuzione è limitato a ciò che si detiene allora, e annullato quando non resta nulla da ridurre.
- **Post only**: un limit rifiutato al piazzamento quando verrebbe eseguito subito contro il bid o l'ask correnti (l'ultimo prezzo senza quotazione).
- Niente iceberg, scelta del trigger, fuori orario regolare, close-position né algo: un prezzo in tempo reale senza profondità, sessione o mark non ha nulla su cui farli giocare, perciò il ticket non li offre.
- Il prezzo di esecuzione è arrotondato al tick dello strumento quando è noto.
- **Commissione** per esecuzione = per ordine + per unità × quantità + % × valore negoziato, portata al minimo. È addebitata nella valuta dello strumento.

### Validità

- **Day** (predefinita) scade alla successiva **mezzanotte UTC**, non alla chiusura della borsa.
- **GTC** non scade mai. **GTD** scade alla data e ora indicate.
- **IOC** e **FOK** vengono annullati se il primo prezzo che incontrano non li esegue. A mercato chiuso, attendono quel primo prezzo. Paper esegue per intero, quindi i due finiscono allo stesso modo.

La scadenza viene verificata circa ogni 10 secondi; l'ordine risulta allora scaduto.

### Bracket e OCO

- Un **take profit** o uno **stop loss** attende l'esecuzione del suo ingresso, poi diventa attivo. Se l'ingresso viene annullato, rifiutato o scade, le sue uscite vengono annullate.
- In un **OCO**, un'esecuzione annulla l'altro ordine del gruppo, e le sue uscite.
- Prezzi, quantità e validità di un ordine attivo si possono cambiare. **Chiudi posizione** invia un ordine a mercato GTC, poi annulla le uscite della posizione.

## Fondi e leva

- Una posizione impegna il suo valore ÷ leva. I future impegnano invece il margine per contratto (vedere sotto) e ignorano la leva.
- Un ordine che apre o aumenta una posizione viene verificato all'invio e di nuovo al prezzo di esecuzione, commissione inclusa. Viene rifiutato se il conto impegnerebbe più della sua equity:

  > not enough funds: the positions would tie up X USD and the account's equity is Y USD

- Un ordine che riduce una posizione è sempre accettato.
- Con **Short selling** a `no`, una vendita che andrebbe allo scoperto viene rifiutata.
- **Non ci sono margin call né liquidazioni forzate**: l'equity può scendere sotto il margine in uso.

## Future

Un future ha bisogno dei dati del contratto, nelle **Impostazioni dello strumento** del grafico:

- **Dimensione del tick, valore del punto e valuta.** Se vuoti, vengono compilati dalle specifiche open source dei contratti di [QuantConnect Lean](https://github.com/QuantConnect/Lean), incluse nell'applicazione e aggiornate ogni settimana. La finestra indica i campi compilati; salvi per conservarli, o digiti i suoi. I valori salvati da lei prevalgono sempre.
- **Margine per contratto (paper)**, nella valuta dello strumento: usi il margine iniziale del suo broker. Oppure spunti **Conta il contratto al suo valore pieno**: quantità × prezzo × valore del punto.

Un ordine su un future senza margine viene rifiutato, e il grafico propone **Imposta margine** (apre le impostazioni dello strumento) o **Usa valore pieno**. Entrambe reinviano l'ordine.

Un ticker è ridotto alla radice del suo contratto (`MNQZ6`, `ESZ25`, `@ES`, `ES=F`, `/ES`). Una radice quotata su più borse senza borsa indicata è un errore che nomina le candidate: **il mercato non viene mai indovinato**. Un future o un'opzione senza valore del punto viene rifiutato, così come ogni strumento senza valuta, tranne una coppia forex, la cui valuta si legge dal nome (`EURUSD` è in USD).

## Valori del conto

La scheda **Conto** del pannello ordini e il pannello delle Impostazioni mostrano:

| Valore | Calcolo |
|---|---|
| **Liquidità** | depositi − prelievi + realizzato − commissioni |
| **Risultato aperto** | Σ (prezzo − prezzo medio) × quantità × valore del punto |
| **Equity** | liquidità + risultato aperto |
| **Realizzato** | dall'ultima reimpostazione, prima delle commissioni, a costo medio |
| **Commissioni** | totale delle commissioni |
| **Margine iniziale** | margine per contratto dei future + valore delle altre posizioni ÷ leva |
| **Disponibile** | equity − margine iniziale, mai sotto zero |
| **Valore delle posizioni** | esposizione lorda, lunga e corta |

La scheda si aggiorna ogni 5 secondi finché è aperta.

### Altre valute

Il conto ha una sola valuta. Uno strumento in un'altra viene convertito al **tasso di cambio del giorno del journal**: risultati realizzati e commissioni al tasso della loro esecuzione, risultati aperti e margine a quello di oggi.

- Un ordine viene rifiutato se non esiste un tasso: `no EUR to USD rate: add one in the journal's FX settings, or trade in USD`.
- Una posizione il cui tasso sparisce in seguito viene indicata come esclusa dai totali.

## Nel journal

Le esecuzioni paper si importano come quelle di qualsiasi broker: **Journal → Importa → Da un broker**, scelga il conto paper, il book e il periodo, anteprima, importa. Vedere [Importare operazioni nel journal](/it/config/brokers#import-trades-into-the-journal). Anche la **sincronizzazione automatica** funziona.

## Da sapere

- Gli importi sono tenuti come decimali esatti, come li registra una borsa: 0,1 acquistato tre volte e 0,3 venduto non lascia nulla di aperto, e un prezzo deve cadere esattamente sul tick dello strumento.
- Liquidità, ordini ed esecuzioni sono salvati nel database e sopravvivono ai riavvii. I prezzi in tempo reale no: dopo un riavvio, le posizioni sono valutate al prezzo medio fino alla quotazione successiva.
- Eliminare il conto elimina la sua liquidità, i suoi ordini e le sue esecuzioni.
- Ogni chiamata d'ordine è nel [registro di audit](/it/config/trading#audit-log).
- Non modellati: posizione in coda, liquidità di mercato, esecuzioni parziali, interessi, costi di prestito, funding, dividendi.
- Ordini e movimenti di liquidità paper non sono esposti agli [agenti IA (MCP)](/it/config/ai-agents), e i conti paper sono disattivati in [modalità demo](/it/guide/demo).
