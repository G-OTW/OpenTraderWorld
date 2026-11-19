# Trading Journal

Registra ogni operazione, in qualsiasi valuta, e ottieni statistiche di performance oneste: curva di equity, win rate, expectancy, profit factor, drawdown, Sharpe e altro. Il journal è organizzato in undici schede: **Breakdown**, **Analytics**, **PnL Calendar**, **Trades**, **What-if lab**, **Strategies & capital**, **Tags**, **Templates**, **Fees & currency**, **Import**, **Pending tasks**.

## Categorie

Le operazioni vivono in **categorie**: cartelle come *Crypto scalping* o *Long-term stocks*, ciascuna con il proprio colore, capitale e statistiche. Creale dalla barra delle categorie; trascina per riordinare. Eliminare una categoria elimina le sue operazioni. La categoria **predefinita** non si può eliminare.

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

Le operazioni mantengono la valuta in cui le hai inserite. Il selettore **Visualizzazione** nell'intestazione della pagina di Breakdown, Analytics, PnL Calendar, Trades e What-if lab sceglie la valuta in cui vengono convertite, con un feed FX giornaliero che recupera automaticamente i cambi ogni giorno lavorativo, riportando i tassi in avanti su weekend e festivi.

Un cambio viene cercato in quest'ordine:

1. La fonte online (tassi di riferimento BCE), che copre le valute principali.
2. Una serie giornaliera salvata in [Historical Data](/it/modules/market-data#histdata), per le valute che la fonte online non quota (USDT, USDC). Le coppie consuete sono provate per impostazione predefinita. Quando mancano, **Indica il mercato** in **Pending tasks** prende una serie già scaricata, oppure un connector e la coppia come la scrive quel venue (Binance: `USDTUSD`), e quel mercato viene usato al loro posto su ogni data. **Scarica una serie** ne preleva una quando non c'è ancora nulla di salvato.
3. Un cambio che digiti a mano.

Se nessuna fonte riesce a dare un prezzo per una data, quelle operazioni sono **escluse dai totali convertiti** e compaiono in **Pending tasks**, dove inserisci a mano i cambi mancanti basati su USD (1 USD = … di quella valuta) e le operazioni tornano a contare. Un cambio digitato a mano prevale sempre.

**Pending tasks** elenca anche gli strumenti che una [sincronizzazione dal broker](#pull-from-a-broker) ha trattenuto, perché tu risponda.

## Breakdown (le tue statistiche)

Per categoria o su tutte, filtrabili per intervallo di date, ticker, side, classe di asset, strategia, segnale e tag. La barra dei filtri è condivisa con Analytics, quindi un ambito impostato in una schermata è l'ambito dell'altra, e l'elenco dei ticker offre i simboli che il journal contiene davvero:

- **Dettagli**: su una categoria, un riquadro richiudibile in alto contiene una descrizione modificabile (la sua tesi, le regole, l'ambito).
- **Curva di equity** nella valuta di visualizzazione o in percentuale del capitale, con i pulsanti di periodo **7G**, **1M**, **6M** e **1A** (clicca di nuovo quello attivo per tutto lo storico).
- Capitale investito · Equity attuale · PnL realizzato · Rendimento · Win rate · Operazioni (chiuse/aperte) · Expectancy · Profit factor · Vincita media / Perdita media · Miglior / Peggior operazione · Max drawdown · Sharpe e Sortino · Fee totali · Margine di picco · Rendimento sul margine.
- **Sharpe e Sortino** sono calcolati sui rendimenti giornalieri rispetto all'equity portata in ogni giorno, annualizzati, con la stessa definizione usata da Analytics, così le due schermate concordano.

## Analytics (leggere il book)

L'intero book in dieci schede, con gli stessi filtri del Breakdown:

- **Overview**: expectancy in **R** e R totale (sulle operazioni che portano uno stop pianificato), rischio per operazione, Sharpe con Sortino accanto, max drawdown con i giorni passati sotto il picco, giorni di trading vinti e persi, giorno medio, serie attuale e migliore, poi la **distribuzione degli R** e il costo dei tuoi errori taggati.
- **Distributions**: PnL netto per tempo di detenzione, ora di ingresso, giorno della settimana di ingresso e dimensione della posizione, e il numero di operazioni per fascia di profitto. Quale ora della tua giornata paga davvero.
- **Behavior**: cosa fai intorno al tuo vantaggio, e cosa costa. Concentrazione del profitto, dimensione dopo una serie di perdite, ritmo dopo una perdita, come decade la giornata, e l'operazione dopo una vincita contro quella dopo una perdita. Vedi [Behavior analytics](#behavior).
- **Market data**: le candele dietro le tue operazioni. MAE e MFE, efficienza di uscita, cosa è rimasto sul tavolo, distanza dello stop in ATR, e risultati divisi per regime di volatilità e per trend all'ingresso. Vedi [Arricchimento con dati di mercato](#market-data).
- **Open risk**: l'unica scheda sul presente. Cosa è ancora in gioco, dove è concentrato quel rischio, e se cinque linee aperte sono cinque scommesse o una. Vedi [Open risk](#open-risk).
- **Scatter**: due valori qualsiasi delle operazioni uno contro l'altro (data, numero operazione, netto, PnL cumulativo, rendimento sul nozionale, R, tempo di detenzione, dimensione...), colorati per risultato, side, strategia, ticker o classe di asset, con linea di tendenza e zoom.
- **Dispersione 3D**: tre valori delle operazioni su tre assi, più la sessione di mercato dell'ingresso, in un cubo che ruoti e ingrandisci a mano. Mostra se due variabili contano solo insieme.
- **Serie 3D**: ogni serie vincente e perdente disegnata operazione per operazione, in denaro o in R, da zero, così le loro forme si confrontano direttamente.
- **Breakdown**: performance raggruppata per strategia, simbolo, tag, classe di asset o side, con operazioni, win rate, netto, expectancy, R medio e profit factor per riga.
- **Compare**: questo giorno, settimana, mese, trimestre, anno o un intervallo personalizzato contro quello appena precedente, riga per riga (netto, operazioni, win rate, expectancy, R medio, profit factor, max drawdown, fee, giorni di trading), sopra una striscia degli ultimi dodici periodi.

Le operazioni chiuse senza cambio FX per la loro data vengono contate a voce alta invece di essere scartate in silenzio.

## Analisi del comportamento {#behavior}

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

- **Discover** legge ciò che le operazioni filtrate richiedono rispetto a ciò che hai già salvato, e non scrive nulla. Per strumento ottieni le operazioni nell'ambito, le barre in archivio, le finestre mancanti e uno stato: *ready*, *partial*, *missing*, *no source*, *unsupported*, *contract needed*, *no dates*.
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

## What-if lab {#lab}

Il **What-if lab** rigioca le operazioni chiuse della categoria aperta con un solo cambiamento e mette il risultato accanto a ciò che hai fatto davvero: le stesse cifre prima e dopo (operazioni, netto, fee, win rate, per operazione, profit factor, max drawdown, giorno peggiore), le due curve cumulative sullo stesso asse, e ogni operazione con la sua variazione. Dove il cambiamento esclude un'operazione, la curva resta piatta.

Una scheda per cambiamento:

- **Exits**: altri stop, target e segnali di uscita, vedi [più sotto](#exit-lab).
- **Size**: ogni operazione da 0,5 a 3 volte la sua dimensione, con le fee scalate di conseguenza. Un **Daily loss cap** opzionale blocca i nuovi ingressi per il resto della giornata appena la perdita realizzata lo raggiunge.
- **Filter**: tenere o scartare le operazioni aperte in una finestra oraria di ingresso (ore, giorni della settimana), e tenere solo quelle in cui una condizione su un indicatore era vera sull'ultima candela chiusa all'ingresso. Solo indicatori integrati, con gli stessi calcoli dei backtest e degli alert sul grafico.
- **Reverse**: gli stessi ingressi e uscite con il side invertito, su ogni operazione o solo su quelle che passano un filtro. Le fee restano quelle pagate.
- **Fees**: le fee pagate rispetto a una delle tue [strutture di fee](#fee), addebitate come le addebita il form dell'operazione. Con una famiglia di contratti (future micro e standard), le operazioni passano all'altro contratto, e i contratti che non formano un contratto intero mantengono le proprie fee.

Niente viene indovinato. Un'operazione senza tasso FX, senza orario di ingresso per una finestra o senza candela salvata per una condizione è elencata come **non misurata**, con il motivo, e lasciata fuori da entrambi i lati. Il verdetto in una frase compare da 20 operazioni chiuse. Sotto quella soglia, leggi le cifre come un abbozzo.

### Exits {#exit-lab}

La scheda **Exits**, che si apre anche da **Segnali SL/TP** nella scheda Trades, rigioca le tue operazioni chiuse con regole semplici di stop, target e uscita (stop in percentuale, in ATR e trailing, target in R, uscite su RSI, Bollinger, media mobile e MACD, uscite a tempo...). Ingresso, dimensione e fee restano i tuoi, si sposta solo l'uscita, quindi ogni cifra è il prezzo dell'uscita. Legge le candele salvate dalla scheda [Market data](#market-data), al massimo 500 operazioni per analisi.

- **Cosa viene testato**: ogni regola predefinita del catalogo in una sola esecuzione (28 in tutto), più ogni segnale portato con **Open in lab**.
- **Come si applica un segnale**: *Aggiunto alla mia uscita* lascia che un segnale chiuda un'operazione solo prima della tua uscita reale. *Sostituisce la mia uscita* fa del segnale l'uscita, e **Barre dopo l'uscita** stabilisce per quante candele oltre la tua uscita reale può ancora chiudere l'operazione (20 per impostazione predefinita, al massimo 500).
- I segnali sono classificati per PnL netto, drawdown più basso, win rate, R medio o profit factor rispetto a **Le mie uscite reali**, con il miglior stop, il miglior target e il miglior stop + target in evidenza, le curve cumulative e le operazioni dietro ogni riga.

### Scheda di valutazione della strategia

**Report**, su una strategia in **Strategies & capital**, riassume le operazioni chiuse che la portano: operazioni, netto, per operazione, win rate, R medio, profit factor e max drawdown, poi la distribuzione degli R e il risultato per ora di ingresso, giorno della settimana, regime di volatilità e strumento.

Sotto, quattro what-if standard girano sulle stesse operazioni: uno stop al movimento avverso mediano delle operazioni vincenti, un target al 75° percentile del movimento favorevole, l'ora di ingresso peggiore eliminata, e 1,5 volte la dimensione, sotto il daily loss cap che inserisci sulla scheda (nessuno per impostazione predefinita). Lo stop e il target richiedono le escursioni misurate da [Market data](#market-data). **Open in lab** apre uno qualsiasi di essi nel lab con le impostazioni già compilate.

## Insights {#insights}

Il pulsante **Insights**, nell'intestazione del journal, su un'esecuzione di backtest salvata e sulla dashboard di un [bot di trading](/it/modules/robots), apre una pagina in cui un insieme fisso di ricercatori legge le operazioni chiuse. Ogni risultato è una frase, con le sue prove: quante operazioni, e cosa cambia nel risultato, nel drawdown o nel rischio per operazione.

Cosa cercano:

- **Ore e giorni della settimana**: l'ora o il giorno di ingresso peggiore, e il migliore, con una dimensione di 1,5 volte.
- **Tempo di detenzione**: perdenti tenute più a lungo delle vincenti, o vincenti lasciate correre.
- **Posizionamento dello stop**: stop più stretti di quanto richiedano i movimenti avversi delle vincenti, o più larghi del necessario.
- **Concentrazione**: un ticker o una classe di asset che zavorra il book o porta la maggior parte delle operazioni. Il settore è usato solo quando per l'azienda è salvato un codice di settore SEC.
- **Fee**: quanto avrebbe addebitato una delle tue strutture di fee, anche sull'altro contratto di una famiglia. Senza struttura, la pagina lo dice e rimanda a Fee.
- **Dimensione dopo le perdite** (solo esecuzioni di backtest e bot, la sezione [Behavior analytics](#behavior) del journal lo copre già): la dimensione della posizione dopo due perdite di fila rispetto alla dimensione dopo una vincita.
- **Long contro short**: un side perde mentre l'altro guadagna.

Quanto fidarsi:

- Sotto le 20 operazioni chiuse non viene affermato nulla. Al di sotto, la pagina propone il passo successivo: importare o registrare operazioni, eseguire un backtest più lungo, o lasciare che il bot operi più a lungo. Un risultato deve anche spostare almeno il 5 % del volume (la somma dei risultati netti in valore assoluto), con abbastanza operazioni su ciascun lato del confronto. *Nothing stands out* è una risposta vera.
- Sul journal, le affermazioni di behavior e di open risk sono elencate accanto ai risultati, invariate.
- **Open in lab** apre il what-if lab del journal con il cambiamento già compilato, così puoi verificare l'affermazione operazione per operazione. Su un'esecuzione o un bot, un risultato su un'ora o un giorno della settimana offre **Add as filter**: la strategia si apre con quella finestra di trading come bozza non salvata. Restringe sempre e solo la finestra.
- Su un'esecuzione o un bot, gli importi sono nella valuta del suo conto, e le strutture di fee a importo fisso non vengono conteggiate.
- Un'esecuzione di backtest deve essere salvata prima. La fonte journal richiede il modulo Trading Journal, le esecuzioni richiedono Backtest e i bot richiedono Bot di trading.

## Tag di disciplina

Un **tag** è una regola che hai infranto o rispettato: *ho spostato lo stop*, *nessun setup*, *ho aumentato dopo una perdita*. Spuntali sulle operazioni a cui si applicano e Analytics li prezza: quante operazioni chiuse hanno infranto una regola, cosa fanno in media rispetto a quelle pulite, e il divario tra le due. Questo è il **costo degli errori**, in denaro.

## PnL Calendar

Una griglia mensile del PnL realizzato giornaliero, verde per i giorni in positivo e rosso per quelli in negativo, scalata sul giorno più grande del mese, con i totali settimanali a lato. Il selettore di unità lo mostra nella valuta di visualizzazione o in percentuale del capitale, ogni giorno letto rispetto al capitale portato in quel giorno (capitale investito più ciò che era stato realizzato prima). Clicca un giorno per aprire il suo [report del giorno](#day-report).

Legge anche le tue **routine di trading**. Collega le routine che una categoria segue, per un periodo, e ogni giorno di trading porta un puntino: verde quando ogni routine dovuta quel giorno è stata spuntata, rosso quando nessuna lo è stata, ambra nel mezzo. Un giorno in cui non hai fatto trading resta grigio qualunque cosa dicano le routine, e un giorno senza alcuna routine dovuta non ottiene alcun puntino. Passandoci sopra si nomina ogni routine con il proprio segno.

Le routine stesse vivono in [Trading Routines](/it/modules/productivity#routines) e si spuntano lì: il journal registra solo quali un book esegue, così la stessa abitudine non viene mai scritta o spuntata due volte.

Legge allo stesso modo i tuoi check-in di [Mindset](/it/modules/productivity#mindset). **Mindset**, sul calendario di una categoria, sceglie i check-in che quel book segue. Ogni giorno porta allora il loro punteggio, e un giorno di trading in cui nessuno è stato compilato è segnato come **mancato**. Un check-in saltato in un giorno senza trading non è una mancanza. Clicca il badge per aprire il check-in di quel giorno in Mindset.

Routine e Mindset sono anche a un clic di distanza nella barra laterale del journal.

### Report del giorno {#day-report}

Un giorno del calendario apre il suo report, con frecce verso il giorno di trading precedente e successivo:

- **Riquadri**: P&L netto con lordo e commissioni, win rate, profit factor ed expectancy, payoff, drawdown intraday, R totale e durata media.
- **P&L intraday**: il risultato realizzato del giorno, operazione dopo operazione, con ogni operazione al passaggio del mouse.
- **Dettagli del giorno**: miglior e peggior trade, esecuzioni, posizioni ancora aperte, dalla prima all'ultima esecuzione e trade segnati come errore, poi il giorno per strumento.
- **Posizioni** del giorno e gli **Ordini eseguiti** in quel giorno.
- **Routine** e **Mindset**: le routine dovute quel giorno e i check-in compilati, ciascuno apre la propria scheda.

**Modifica nella lista dei trade** apre le operazioni del giorno per modificarle. La pagina del check-in di Mindset rimanda qui con **Giorno nel diario**.

## Importare un trade book {#import-a-trade-book}

La vista **Import** prende un trade book che tieni altrove (un export del broker, un altro journal, un foglio di calcolo) e lo trasforma in operazioni del journal. Nessun parser per broker: CSV, TSV, JSON e Parquet passano tutti dallo stesso rilevamento.

**Come funziona.** Trascina il file, il server propone una mappatura, la controlli con un'anteprima di operazioni reali, poi importi.

- Il **rilevamento** legge le intestazioni (en, fr, es, de, it, pt, più la terminologia comune dei broker) *e* i valori stessi. Delimitatore, separatore decimale e date giorno-prima vs mese-prima sono decisi per colonna. Una colonna che non riesce a identificare con confidenza resta **non mappata** invece di essere indovinata, e la assegni tu.
- **Fuso orario.** **Gli orari sono in** sceglie il fuso in cui sono stati scritti gli orari del file: un fuso con nome che segue l'ora legale, o uno scostamento fisso. Per impostazione predefinita è il fuso del tuo browser.
- **Anteprima prima di scrivere.** Il passo di analisi non scrive nulla: restituisce le operazioni costruite, i totali e gli errori per riga, e si riesegue a ogni modifica della mappatura, quindi ciò che vedi è esattamente ciò che verrà salvato. Il P&L del file stesso è confrontato con quello calcolato.
- **Forma della riga.** Una riga è o un **round trip** (una riga = un'operazione) o un'**esecuzione** (una riga = un fill). Le esecuzioni sono raggruppate per strumento in posizioni con gambe di ingresso e uscita; ciò che è ancora aperto alla fine viene importato come operazione aperta.
- **Estratti impilati.** Un export che impacchetta più tabelle in un file (lo stile IBKR) viene letto sezione per sezione, con un selettore per cambiare tabella o leggere il file piatto.
- **Point value.** Nessun export riporta quanto vale un punto di uno strumento. Spunta **Imposta un valore del punto per ticker** per inserirlo per ogni ticker trovato nel file (future, CFD); se lasciato disattivato, ogni ticker conta 1, che è giusto per azioni e crypto. Viene salvato con la mappatura.
- **Mappature.** Salva una mappatura e il file successivo dalla stessa fonte è riconosciuto dall'impronta delle sue intestazioni e si mappa da solo. Anche le colonne che correggi a mano vengono ricordate.

::: tip Una mappatura non è un template
Un **template** del journal è il modulo con cui registri a mano un'operazione. Una **mappatura** di import dice quale colonna di un file esterno è quale campo dell'operazione. Sono elencati separatamente e mai mescolati.
:::

### Preleva da un broker {#pull-from-a-broker}

Con un [conto broker](/it/config/brokers) concesso al journal, **Preleva da un broker** fa lo stesso import senza file: scegli l'account e un periodo, e i fill tornano integrati in posizioni, in anteprima prima che venga scritto qualcosa. Non c'è mappatura da controllare, un'API risponde con campi tipizzati. Rieseguire un periodo più ampio aggiorna le posizioni già importate invece di raddoppiarle. Spunta **Sincronizza automaticamente** e la sincronizzazione si ripete da sola alla frequenza scelta, da dove si è fermata l'ultima; la pianificazione si mette in pausa, si riprogramma o si elimina in **Automator → Job**. **Pianifica sincronizzazione** nella scheda Trades apre lo stesso pannello.

- **Verso** sceglie la categoria in cui arrivano le operazioni. Il periodo si digita, o si imposta con un clic: **7 giorni**, **30 giorni**, **90 giorni**, **1 anno**.
- **Strumenti** è facoltativo per la maggior parte dei broker (vuoto prende ogni strumento del periodo) e obbligatorio per un broker che risponde uno strumento alla volta. **Proponi dal conto** lo compila.
- **Questo conto può andare short**: se lasciato disattivato, una vendita senza nulla di aperto è letta come uno strumento comprato prima del periodo, non come uno short che non hai mai preso.
- **Strumenti trattenuti.** Un fill dice acquisto o vendita, mai apertura o chiusura. Uno strumento i cui fill non tornano viene trattenuto mentre tutto il resto si importa, ed è elencato nel dialogo e in **Pending tasks**. Rispondi con il prezzo di apertura di una posizione aperta prima del periodo, prendi i fill così come sono, o lascia fuori lo strumento. Le tue risposte sono riapplicate da ogni sincronizzazione successiva.

**Annullare un import.** Ogni import è un **batch**, elencato con la sua data, file e numero di operazioni. *Revert* elimina esattamente le operazioni che ha creato. Gli import sono anche deduplicati **per categoria**, quindi reimportare lo stesso file non cambia nulla (lo stesso estratto può comunque alimentare due categorie, dato che una categoria è un book). *Mantieni, scollega* un batch per togliere quella protezione e rendere di nuovo ordinarie le sue operazioni.

## Export e report

**Esporta**, nell'intestazione della pagina di Breakdown, Analytics, PnL Calendar (e del suo report giornaliero) e Trades, esporta i tuoi dati o genera un report di performance. L'**Ambito** è la categoria aperta nella pagina, oppure **Tutte le categorie**. Il **Periodo** è **Settimanale** o **Mensile**, con le frecce per passare al periodo precedente o successivo:

- **CSV delle operazioni**: le operazioni grezze, per fogli di calcolo o software fiscale. Offre anche **Tutto lo storico**.
- **Report di performance**: win rate, expectancy, fee, ripartizione per strategia e categoria, curva di equity, reso in **Markdown o PDF**.

## Funziona con

- **Tax Calculator**: carica il PnL realizzato del journal per un anno fiscale, ripartito in guadagni capitale / derivati / crypto.
- **Charts**: il selettore **Journal** nel ticket d'ordine del grafico collega lo strumento a una categoria del journal, o ne crea una con **Nuovo journal**. A ogni riconciliazione del conto di trading, i fill del broker su quello strumento vengono registrati lì come posizioni, chiunque abbia inviato l'uscita. Una posizione resta nel journal in cui è stata aperta, e ciò che il conto deteneva prima del collegamento non viene mai registrato.
- **Historical Data**: le schede Market data e Open risk leggono le candele tramite i [data connector](/it/config/connectors) concessi al journal, e scaricano ciò che manca come normali job.
- **Trading Routines**: il PnL calendar mostra, per giorno di trading, se le routine seguite dalla categoria sono state spuntate.
- **Mindset**: il PnL calendar mostra il punteggio del check-in di ogni giorno e segnala i giorni di trading in cui manca.
- **Dashboard**: un widget quick-trade registra un'operazione dalla home, e i widget **Journal analytics**, su una categoria o su tutte, mostrano PnL netto, win rate, equity curve, posizioni aperte, osservazioni sul comportamento e altro.
- **RemindMe**: aggiungi un promemoria dalla pagina del journal (ad es. revisione settimanale).
