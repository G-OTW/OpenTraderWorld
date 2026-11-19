# Bot di trading {#trading-robots}

Un **bot di trading** è una strategia di Backtest eseguita in avanti. Congela le impostazioni della strategia, gira a ogni chiusura del timeframe della strategia su un insieme di strumenti, e simula i suoi fill oppure invia i suoi ordini a un conto broker. Oltre alla strategia, ogni bot ha il proprio denaro (una **politica di capitale**), delle **esclusioni**, delle **protezioni** che reagiscono ai suoi stessi risultati, e una **priorità**.

Non c'è un secondo motore: ogni esecuzione legge la finestra con il normale backtest e agisce solo sulla candela appena chiusa. Un bot a segnali porta avanti le posizioni che ha preso così come le ha prese; ciò che la finestra mostra sulle candele precedenti è storia, mai registrata né inviata.

La strategia fornisce i segnali, il suo stop e il suo target, e i suoi tipi di ordine. Il bot fornisce il denaro e il calendario: la sua [politica di capitale](#capital-policy) e le sue [esclusioni](#exclusions). Lo strumento (valore del punto, tick di prezzo, passo di dimensione, minimo) è quello del venue. Il capitale, la dimensione, i filtri di giorno e di sessione e i costi della strategia non vengono letti.

Il modulo richiede l'installazione di **Backtest**. Si trova su **/robots**, come **Bot di trading** nel selettore dei moduli.

## La flotta {#the-fleet}

La pagina della flotta elenca ogni bot: schede su un telefono, una tabella ordinabile su un desktop. Si aggiorna ogni 10 secondi mentre la pagina è visibile, e i conti alla rovescia avanzano ogni secondo.

- **Cifre chiave**: **Bot** (con quanti sono attivi), **Capitale** (ripartito per bot), **Oggi** (il P&L di oggi, e gli eventi non letti), **Aperte** (le posizioni aperte, e quanti bot ne detengono una).
- **P&L della flotta**: la curva di equity sommata dei bot, con **P&L**, **Realizzato** e **Non realizzato**. Filtrala per ambiente (**Tutti gli ambienti**, Simulato, Carta, Testnet, Reale) e per una data **Dal** e **Al**. Senza data di fine, l'ultimo punto è valorizzato sulle posizioni aperte.
- **Classifica**: i bot ordinati per il P&L dei trade chiusi **Oggi**, su **1S**, **1M** o **Tutto**, con il rendimento sul capitale iniziale.
- **Esposizione per tipo di asset**: ciò che le posizioni aperte impegnano all'ultimo prezzo, ripartito per tipo di asset (cripto, futures, forex, azioni...), con quanti bot su ciascuno.
- **Filtri di stato**: un clic su un chip mostra i bot in quello stato (**Attivo**, **Fermato da una protezione**, **Fermo**, **Disarmato, da riarmare**, **Degradato**, **Bloccato**, **Broker non raggiungibile**, **Errore**), con un conteggio su ciascuno. Un secondo clic lo toglie. Il selettore del conto restringe a **Simulato** o a un solo conto.
- **Da verificare**: i bot con un errore, una protezione che li trattiene o eventi non letti, ciascuno con il motivo.
- **Prossimi tick**: gli otto bot in scadenza più vicina, con i loro strumenti e un conto alla rovescia.
- **Segna N come letti** azzera gli eventi non letti di ogni bot. **Nuovo bot** apre la procedura guidata.

## Creare un bot {#create-a-robot}

**Nuovo bot** nella pagina della flotta, oppure **Avvia come bot** in Backtest: su un risultato (l'esecuzione esattamente com'era) o su una strategia salvata nella libreria. In entrambi i casi si apre la procedura guidata, un passaggio alla volta:

1. **Strategia**: il nome del bot, e una strategia salvata da congelare (o l'esecuzione da cui proviene). **Crea una strategia** apre Backtest. Una modifica successiva della strategia non cambia nulla nel bot finché non lo aggiorni (vedi [Impostazioni](#settings)).
2. **Strumenti**: **Aggiungi uno strumento** apre il selettore di dati del grafico, che cerca nella ricerca di simboli di ogni provider. Non serve alcuno storico salvato: ogni esecuzione preleva le candele che la strategia legge. Fino a 8 strumenti, su un unico **Timeframe** che condividono tutti. Ogni strumento è letto attraverso la fonte di dati da cui è stato scelto, che **Impostazioni del bot** può cambiare in seguito. Dove il provider trasmette un prezzo live, stop, target e limit scattano su di esso; altrove sono controllati alla chiusura della candela. Le impostazioni del bot indicano quale dei due vale per ogni strumento.
3. **Conto**: **Simulate** (nulla viene inviato da nessuna parte) o **Broker account** (vedi [Su un conto broker](#on-a-broker-account)).
4. **Capitale**: la [politica di capitale](#capital-policy).
5. **Protezioni**: le [protezioni](#guards). Un nuovo bot parte con il set predefinito.
6. **Pianificazione e notifiche**: il bot gira a ogni chiusura delle sue candele e decide sulla candela appena chiusa. Il passaggio contiene le sue [**Esclusioni**](#exclusions), un **Rapporto di stato** facoltativo (**Invia un rapporto di stato**, poi quando parte: ogni N minuti, ogni giorno, ogni settimana, ogni mese o una volta, in un fuso orario), **Candele mobili** (quanta storia ogni esecuzione passa al motore, 5000 per impostazione predefinita), **Priorità**, **Notificare** (**Ogni esecuzione**, **Solo su una nuova operazione**, **Mai**) e **Canali**.
7. **Riepilogo**, poi **Crea il bot**.

Il server ricontrolla tutto quando il bot viene creato. Un rifiuto viene mostrato così come è stato formulato, e nomina cosa cambiare.

Il bot parte subito. La finestra **Avvio** *nome del bot* segue la sua prima esecuzione mentre scarica il suo storico e apprende il suo book; **Continua in background** la chiude e apre la pagina del bot mentre l'esecuzione prosegue. Una volta pronto, un bot simulato gira a ogni chiusura da quel momento in poi, e un bot su broker offre **Arm** nella stessa finestra (vedi [Su un conto broker](#on-a-broker-account)). **Apri il bot** apre la sua pagina.

I canali sono i canali di notifica concessi a Bot di trading; il pulsante dei canali nel passaggio li aggiunge o li gestisce senza lasciare la procedura guidata. Senza alcun canale scelto, il bot invia a ogni canale concesso a Bot di trading; senza canali concessi, non viene inviato nulla e ogni evento è comunque registrato nel log del bot. **Notificare** **Mai** non invia nulla. Il rapporto di stato riporta lo stato del bot, le sue cifre dall'ultimo rapporto e ciò che detiene; non viene inviato mentre il bot è escluso e non detiene nulla. **Impostazioni del bot** contengono anche gli avvisi su entrate e uscite per strumento, e la tua formulazione per i messaggi di entrata e di uscita, e per il riepilogo quando un rapporto di stato è attivo.

### Esclusioni {#exclusions}

Le esclusioni sono periodi in cui il bot non prende nessuna nuova entrata. Le uscite sono comunque gestite al loro interno. Sono lette nel fuso orario del bot, e qualsiasi combinazione funziona:

- **Giorni interi**: i giorni della settimana scelti.
- **Fasce orarie**: da un orario a un altro, la fine esclusa; una fascia che finisce prima di iniziare prosegue oltre la mezzanotte.
- **Intervalli di date**: da una data a un'altra, entrambi i giorni inclusi.

Fino a 20 fasce orarie e 20 intervalli di date. Con **Uscire dal mercato**, il bot chiude le sue posizioni quando un periodo inizia, e torna a operare quando finisce. L'entrata e l'uscita da un periodo sono nel log, e vanno ai canali del bot quando detiene una posizione.

## La pagina di un bot {#a-robots-page}

L'intestazione tiene sotto gli occhi lo stato del bot: il suo nome, la strategia e la sua versione, i badge per lo stato, l'ambiente di un bot su broker (**Carta**, **Testnet**, **Reale**), la protezione (in pausa, fermo, o la riduzione come x0.5) e, su un conto broker, **Armato** o **Disarmato**. Sotto: la prossima valutazione come conto alla rovescia, l'ultima valutazione e l'ultima sincronizzazione.

- **Esegui ora** esegue subito il bot. **Ferma la strategia** mette in pausa la sua pianificazione; non chiude le posizioni né cancella gli ordini. **Avvia la strategia** la riprende.
- **Insights** legge le operazioni chiuse del bot, vedi [Insights](/it/modules/journal#insights).
- Mentre una protezione trattiene il bot, un avviso dice perché, con **Riprendi**.

La pagina si aggiorna ogni pochi secondi; le posizioni aperte seguono il prezzo live dove il provider ne trasmette uno. Otto schede:

| Scheda | Mostra |
|---|---|
| **Panoramica** | **Capitale**, **P&L realizzato**, **P&L non realizzato**, **Oggi** (P&L e ordini inviati), **Win rate**, **Fattore di profitto**, **Drawdown max**, **Aspettativa**; la curva di equity; l'ultimo prezzo di ogni strumento (live, candela salvata, o non aggiornato); **P&L per strumento** e **P&L per direzione**; il calendario del P&L; un grafico al timeframe della strategia con entrate e uscite. Un bot su broker mostra anche il suo [pannello di esecuzione](#on-a-broker-account) |
| **Posizioni** | strumento, lato, quantità alla dimensione del bot, prezzo medio, prezzo (live o salvato), non realizzato, stop, target ed età |
| **In attesa** | gli ordini di un bot simulato in attesa della prossima apertura; gli ordini di un bot su broker attivi presso il broker |
| **Operazioni** | le operazioni chiuse, 25 per pagina, con la riduzione della protezione dove si è applicata. **Non presi** conta le entrate che il bot ha rifiutato ed elenca le ultime 10, ciascuna con il suo motivo |
| **Statistiche** | le statistiche delle operazioni, la curva di performance, e per le strategie a segnali una dispersione 3D e le serie |
| **Protezioni** | le regole in forma di frasi, cosa trattengono adesso, e la cronologia degli ultimi 100 scatti, ripristini e riprese, dal più recente |
| **Registro** | gli ultimi 200 eventi: entrate, uscite, rifiuti, azioni delle protezioni, errori |
| **Impostazioni** | strategia, conto, pianificazione, priorità, capitale e protezioni, con **Impostazioni del bot**, l'aggiornamento alla versione della strategia, e **Elimina questo bot** |

Il book di un bot inizia con la sua prima esecuzione: viene registrato solo ciò che accade dalla candela appena chiusa. Ciò che la finestra mostra prima è storia, mai nel suo book, nelle sue statistiche o nelle sue protezioni.

### Impostazioni {#settings}

**Impostazioni del bot** modifica il nome, la fonte di dati di ogni strumento (**Predefinita del fornitore**, o un connector concesso a Bot di trading), le esclusioni, il rapporto di stato, la finestra, la priorità, il capitale, le protezioni e le notifiche. La strategia, gli strumenti e il conto sono congelati: un'altra strategia o un altro conto è un nuovo bot. La nota **Seguita in avanti** elenca gli strumenti e i tipi di ordine di entrata e di uscita della strategia: i suoi segnali, lo stop e il target girano così come sono stati congelati, mentre il suo capitale, la dimensione, il calendario e i costi non vengono letti.

**Passa alla versione attuale della strategia** (mostrato quando il bot proviene da una strategia salvata) prende la strategia salvata così com'è ora. Il book del bot viene svuotato e ricostruito alla prossima esecuzione, e un bot armato viene disarmato.

Eliminare un bot elimina il suo book, la cronologia delle sue protezioni e il suo log.

## Politica di capitale {#capital-policy}

Il denaro del bot dimensiona ogni decisione della strategia: la strategia gira sul capitale del bot, non sul proprio. Ogni campo è facoltativo:

| Campo | Significato |
|---|---|
| **Allocazione** | il capitale con cui il bot opera. Vuoto: il capitale e la dimensione della strategia |
| **Valuta** | la valuta del conto, per leggere le cifre |
| **Quota per posizione (%)** | una posizione vale questa quota del capitale del bot. Richiede un'allocazione |
| **Leva** | il bot dimensiona con essa, fino a 125. Vuoto: 1 |
| **Entrate per posizione** | quante entrate può prendere una posizione, fino a 50. Vuoto: 1. Un bot su broker ne prende solo 1 |
| **Max posizioni aperte** | un'entrata oltre questo limite non viene presa |
| **Max ordini al giorno** | entrate contate per giorno UTC. Le uscite non sono mai trattenute |

Ogni entrata viene poi moltiplicata per la riduzione di una protezione e arrotondata per difetto al passo di dimensione del venue (mai per eccesso). Su un conto broker, la **Maximum position (units)** impostata alla creazione rifiuta un'entrata più grande invece di ridurla.

**Un'entrata rifiutata nomina la sua soluzione**, nella scheda Operazioni e nel log:

> the bot already holds 3 position(s), its maximum of 3: the entry on BTCUSDT was not taken. Raise the maximum of positions

Gli altri si leggono allo stesso modo: una dimensione sotto il passo del venue (aumenta l'allocazione, la quota per posizione o la leva) o sotto il suo minimo (aumenta l'allocazione, la quota per posizione o la dimensione della strategia), il massimo di ordini del giorno, una dimensione oltre la posizione massima.

## Protezioni {#guards}

Le protezioni sono regole sulle **operazioni chiuse del bot stesso**, alla dimensione del bot. Fino a 10 per bot. Le uscite sono sempre gestite, qualunque cosa trattenga una protezione.

| Quando | Allora |
|---|---|
| **perdite consecutive**, **vincite consecutive** (un numero) | **pausa** per giorni o ore (da un minuto a 365 giorni) |
| **drawdown %**, **dal picco** o **dall'inizio** | **fermo fino alla ripresa** |
| **perdita % su**, **importo perso su**, **perdite su**, **guadagno % su** una finestra: **oggi**, **questa settimana**, **questo mese**, **dall'inizio** (nel fuso orario del bot) | **ridurre la dimensione**: **Dividi per** più di 1 e al massimo 10, **Ritorno alla dimensione piena** **dopo operazioni**, **dopo vincite**, **dopo giorni** o **a mano** |

Il set predefinito proposto a un nuovo bot: pausa di un giorno dopo 3 perdite consecutive, e dimensione dimezzata dopo un drawdown del 10% dal picco, con ritorno alla dimensione piena dopo 5 operazioni.

- Un bot in pausa o fermo non prende nuove entrate e continua a gestire le sue posizioni. Un bot ridotto dimensiona al ribasso le sue prossime entrate.
- Una pausa termina da sola. Un fermo aspetta **Riprendi**. Una riduzione torna alla dimensione piena secondo la sua regola di ripristino, in un solo passo.
- Una regola di conteggio riparte da zero dal suo ultimo scatto: una pausa dopo tre perdite scatta di nuovo dopo altre tre. Una regola di livello scatta quando il livello viene superato, e di nuovo quando non era più soddisfatto o la sua finestra è passata alla successiva.
- Più regole insieme: un fermo prevale su una pausa, una pausa prevale su una riduzione, e una riduzione scattata con esse si applica quando le entrate riprendono. Una riduzione su una riduzione si moltiplica, mai sotto x0.1.
- Le percentuali sono sul capitale iniziale del bot (l'allocazione, altrimenti il capitale iniziale della strategia). Un drawdown dal picco è sul capitale di picco; una perdita o un guadagno su oggi, questa settimana o questo mese è sul capitale all'inizio di quel periodo.
- **Riprendi** toglie tutto ciò che una protezione trattiene. Modificare le regole conserva ciò che è trattenuto adesso.

Ogni scatto e ogni ripristino sono nella scheda **Protezioni** e nel log, e vanno ai canali del bot.

## Priorità e limiti di frequenza {#priority-and-rate-limits}

Ogni bot ha una **Priorità** da 0 a 1000, 100 per impostazione predefinita. **Il valore più basso va per primo** quando più bot sono in scadenza insieme o condividono un broker. I bot in scadenza alla stessa chiusura girano fianco a fianco, fino a 8 alla volta.

I bot su un conto broker sono sincronizzati ogni 15 secondi, broker per broker, in ordine di priorità:

- un passaggio invia al massimo 10 ordini a un broker, un bot dopo l'altro in ordine di priorità. Uscite e chiusure non sono mai trattenute da quel limite; un'entrata oltre il limite aspetta il passaggio successivo, e il bot lo dice;
- quando un broker rifiuta le richieste per il suo limite di frequenza, **solo i bot su quel broker aspettano**: fino all'ora che ha indicato, o per un passaggio quando non ne ha indicata nessuna. Ogni bot mostra una nota che nomina l'attesa e cosa cambiare (meno bot o strumenti sul conto, o un limite più alto sul suo piano). I bot su altri broker proseguono.

## Su un conto broker {#on-a-broker-account}

Un bot può inviare i suoi ordini a qualsiasi conto con il [trading](/it/config/trading) attivo e concesso a **Bot di trading** in [Impostazioni → Broker](/it/config/brokers), incluso un conto di [paper trading](/it/config/paper-trading). Il badge dell'ambiente dice dove vanno: **Carta** (il broker paper dell'app), **Testnet** (un conto di test o demo di un venue), **Reale** (denaro vero).

Il motore decide a ogni chiusura e il conto esegue. Alla creazione:

- solo una strategia a segnali viene eseguita su un broker, con una sola entrata per posizione (**Entrate per posizione** 1);
- ogni strumento viene cercato presso il conto, che mostra il simbolo e il passo di dimensione trovati. Uno strumento che il conto non sa nominare viene rifiutato: il bot non indovina mai;
- **Exits**: **At the broker** (lo stop e il target restano presso il broker, collegati all'entrata, dove il broker può mantenerli) o **Watched** (controllati sul prezzo live e chiusi a mercato quando superati);
- più bot possono puntare a uno stesso strumento su uno stesso conto, ma solo uno alla volta è armato, così due bot non operano mai una stessa posizione l'uno contro l'altro.

Un bot su broker è **creato disarmato**: nulla viene inviato finché non lo armi con **Arm**, nella finestra **Avvio** *nome del bot* una volta terminata la sua prima esecuzione, o più tardi dalla sua pagina. Armarlo richiede il **trading in background** firmato sul conto (**Firma il trading in background** apre le condizioni da firmare), e il codice del tuo autenticatore quando il [blocco del trading](/it/config/trading#trading-lock) è attivo e il conto non è paper. Dalla chiusura successiva, le entrate partono a mercato e le uscite come decide la strategia, senza che nessuno sorvegli. Ogni ordine passa per i [limiti di rischio](/it/config/trading#risk-limits) del conto ed è nel [registro di audit](/it/config/trading#audit-log).

L'armamento legge prima il conto: viene rifiutato quando il broker non è raggiungibile, o quando il conto detiene meno del denaro del bot (la sua allocazione, altrimenti il capitale della strategia). Ogni bot è controllato per conto suo, quindi più bot possono condividere un conto; un'entrata che il conto non può pagare in seguito non viene inviata, e il log dice di cosa aveva bisogno e cosa aveva il conto.

Il **pannello di esecuzione** nella Panoramica mostra il livello del conto, i suoi limiti, il book del motore accanto a quello del conto, e il prezzo simulato di ogni operazione accanto a quello del broker. Il suo stato:

| Stato | Significa |
|---|---|
| **Disarmed** | nulla viene inviato finché non armi il bot |
| **In sync** | il conto detiene ciò che il bot si aspetta |
| **Degradato** | un'entrata è stata eseguita solo in parte. Il bot non aggiunge esposizione e continua a gestire le sue uscite finché non chiudi tutto o adotti |
| **Stopped, arm it again** | una posizione è stata aperta a mano sullo strumento del bot mentre il bot era flat, oppure la posizione del bot è stata chiusa, aumentata o ridotta a mano. Il bot è stato disarmato e le sue entrate in attesa annullate; anche le uscite di una posizione chiusa sono annullate. Armalo di nuovo per proseguire |
| **Sending paused** | una protezione trattiene le nuove entrate, oppure il trading è sospeso sul conto: viene inviato solo ciò che riduce il rischio |
| **Check at the broker** | un ordine non ha ricevuto risposta: nulla più viene inviato su quello strumento finché non è chiarito |
| **Broker unreachable** | il broker non ha risposto a tre sincronizzazioni di fila (45 s). Nulla viene inviato finché non risponde, e il conto viene riletto prima di inviare qualsiasi cosa |
| **Account error** | il conto non può essere usato (le sue credenziali o il suo permesso): nulla viene inviato finché non è possibile |

Il bot legge il conto a ogni sincronizzazione (ogni 15 s, e per prima cosa dopo un riavvio) prima di inviare qualsiasi cosa. Ogni modifica che fai a mano alla posizione del bot lo ferma finché non lo armi di nuovo. Uno stop o un target che sposti a mano presso il broker resta dove l'hai messo: il bot non lo sposta più. Annulla a mano sia il suo stop sia il suo target e la sincronizzazione successiva li ripiazza, una volta; dopo, il bot li sorveglia sul prezzo live. Annullare un altro dei suoi ordini in attesa non cambia nulla per lui. Il controllo di posizione del conto lascia allo stesso bot lo strumento di un bot: le operazioni di un bot non sospendono mai il conto.

**Posizioni** e **In attesa** mostrano solo ciò che è presso il broker. Un'entrata che non è partita (rifiutata da un limite, da una protezione, dalle regole di dimensione del venue, dai fondi del conto o dal broker) è una riga nel **Registro**, con il motivo esatto al passaggio del mouse. I prezzi e il P&L di un'operazione chiusa sono quelli a cui i suoi ordini sono stati eseguiti presso il broker, commissioni incluse, una volta eseguita la sua uscita; fino ad allora vale la cifra simulata. Uno stop o un target eseguito presso il broker, Stop and flatten, o una chiusura fatta presso il broker chiude l'operazione del bot a quel fill, anche quando le candele del bot non hanno mai raggiunto il livello; un'entrata che non è partita non viene presa. Una candela che il provider corregge dopo la sua chiusura non cambia nulla: il bot fa ciò che ha deciso a quella chiusura, e un'inversione è un solo messaggio.

- **Disarm** interrompe gli invii. Gli ordini già presso il broker restano lì.
- **Stop and flatten** disarma, annulla gli ordini attivi del bot e chiude a mercato ciò che detiene.
- **Adopt the difference** lascia a te ciò che il conto detiene al di fuori del bot, così un bot degradato torna a operare.

Armare, disarmare, chiudere tutto e adottare si fanno sempre da te dalla pagina, mai da un agent o da un workflow.

## Sessioni paper {#paper-sessions}

Le sessioni paper di Backtest ora sono bot, con il loro book, eventi, conto e permessi dei canali. Un vecchio link a una sessione paper apre la pagina del bot. **Avvia come bot** sostituisce **Paper trade** in Backtest.

## Da sapere {#good-to-know}

- Gli [agenti IA (MCP)](/it/config/ai-agents) possono leggere i bot (l'elenco, un bot, la sua dashboard, le sue operazioni, gli eventi delle sue protezioni, il suo pannello di esecuzione, la flotta, la posta in arrivo e le variabili dei messaggi). Creare, modificare, eseguire o armare un bot resta nell'app.
- Un bot legge le candele e il prezzo live di ogni strumento attraverso la fonte di dati scelta per esso, altrimenti un [data connector](/it/config/connectors) concesso a Bot di trading, altrimenti uno concesso a Backtest, altrimenti il primo connector del provider.
- In [modalità demo](/it/guide/demo) il modulo è in sola lettura: due bot precaricati su un conto paper demo, uno ridotto dalla sua protezione e uno in pausa, nessuno dei due armato né in esecuzione.
- Come un bot simulato esegue i fill sul prezzo live, e le opzioni che condivide con il backtest: [Backtest](/it/modules/market-data#backtest).
