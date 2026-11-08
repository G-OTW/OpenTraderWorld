# Dashboard e navigazione

La schermata iniziale dell'app, più le due cose che stanno sopra ogni modulo: la casella di ricerca e la casella delle notifiche.

## Pagine della dashboard

La dashboard offre cinque pagine predefinite, una per modo di lavorare: **Investor** (posizioni di lungo periodo), **Trader** (esecuzione e sessione di mercato), **Quant** (ricerca, backtest e lavoro algoritmico), **Portfolio** (posizioni) e **Utilities** (automazione, note e strumenti). Ognuna parte con un insieme di widget già pronto. Vengono create una volta e poi sono pagine normali: riorganizzale come vuoi. Solo le tue pagine si possono eliminare.

Crei anche **le tue pagine** con **Custom +** nella sezione **Area di lavoro** della barra laterale, che elenca le tue dashboard. Ognuna ha un nome, una descrizione opzionale e un breve **tag** mostrato nella barra dei preferiti. La **barra dei preferiti** in alto contiene da una a cinque pagine, aggiunte o tolte con la stella accanto; la sua prima voce è la pagina su cui si apre la dashboard. Quando la barra è piena, una nuova stella chiede quale preferito sostituire.

Usale come si divide una giornata di trading: una pagina *Morning* con il feed di notizie, il calendario economico e la checklist della routine; una pagina *Positions* con il portafoglio e la watchlist; una pagina *Admin* con todo e timer.

## Modificare un layout

**Personalizza** apre la modalità **Modifica layout**. Una pagina è una griglia libera di 12 colonne: le tessere scorrono da sinistra a destra e vanno a capo sulla riga successiva, e nessuna riga viene salvata. In modalità modifica puoi:

- aggiungere **tessere di modulo** (un link a un modulo, e lo stesso modulo può comparire in qualsiasi numero di pagine) o **widget** con la tessera **Aggiungi**; **Aggiungi widget** nell'intestazione funziona anche fuori dalla modalità modifica;
- trascinare una tessera ovunque: le tessere successive si spostano e la griglia si riorganizza; **ridimensionare** qualsiasi tessera per estensione in colonne;
- impostare l'altezza di una riga (**Compatto**, **Standard** o **Alto**) e aprire la **configurazione** di un widget (l'ingranaggio sulla tessera), dove l'altezza **Titolo** fa del widget una fascia di intestazione a tutta larghezza, per un titolo di sezione in Free text;
- inserire uno **Spaziatore**: uno spazio a tutta larghezza di altezza regolabile.

Le tessere sono link, non copie: rimuoverne una da una pagina non tocca mai il modulo né i suoi dati.

## Widget

Un widget è un'anteprima live e interattiva di un modulo: legge e scrive tramite l'API di quel modulo, quindi ciò che fai nel widget è reale. I widget il cui modulo non è installato semplicemente non vengono offerti.

La maggior parte dei widget esiste in più **varianti**, ognuna una vista diversa degli stessi dati: Net worth, per esempio, offre il valore attuale, la sua variazione, un trend, attività contro passività, una ripartizione per categoria, le attività da rivedere e le attività principali. **Aggiungi widget** mostra ogni variante come una scheda a sé, e **Widget type** nella configurazione del widget la cambia in seguito. La tabella indica cosa copre ogni widget nell'insieme delle sue varianti.

| Widget | Cosa fa |
|---|---|
| **Free text** | Una nota o un titolo che scrivi tu, in markdown-lite. |
| **Notes** | I documenti dell'Editor modificati di recente, quanti ne hai, e un campo di una riga per creare una nota. |
| **News feed** | Ultimi elementi di un feed scelto, come elenco o griglia. |
| **Mailbox** | L'ultima posta non letta, dalla più recente. |
| **Time tracker** | Avvia/ferma un timer di progetto senza lasciare la pagina. |
| **Quick trade** | Scegli una categoria + un modello e apri il modulo di aggiunta operazione. |
| **Journal analytics** | PnL netto, win rate, profit factor, curva dell'equity, posizioni aperte, esposizione, analisi del comportamento e heatmap del PnL giornaliero. |
| **Goals** | Un breve elenco di obiettivi con avanzamento; aggiungine uno inline. |
| **ToDo** | Attività aperte, spuntabili sul posto. |
| **Trading routine** | La checklist di oggi, spuntabile sul posto. |
| **Mindset** | Il check-in del giorno. |
| **Reminder** | Un modulo rapido per aggiungere un promemoria. |
| **Reminder stats** | Notifiche non lette, promemoria in arrivo e scaduti, e quello che scatta più spesso. |
| **Calendar** | Oggi e questa settimana a colpo d'occhio. |
| **Economic calendar** | Prossimi eventi macro, in versione compatta. |
| **Portfolio** | Un riepilogo del portafoglio con valore live. |
| **Subscriptions** | La spesa ricorrente mensile, poi cosa si rinnova per prossimo. |
| **Net worth** | Patrimonio netto attuale, la sua variazione su una finestra che imposti e una sparkline. |
| **Watchlist** | Quotazioni live di una lista scelta: prezzo, variazione 24h e 7g. |
| **Fundamentals** | Serie e bacheche macro, uno snapshot aziendale, una voce di bilancio per trimestre, anno o TTM, aziende ordinabili, filing filtrati, prossimi utili e valutazione rispetto ai peer salvati. Letto dai dati salvati senza consumare quota del provider. |
| **Automator** | Workflow in esecuzione, prossime esecuzioni pianificate, tasso di successo per workflow, errori recenti ed esecuzioni rapide preferite. |
| **Webhooks** | Eventi ricevuti, eventi recenti, tasso di errore per endpoint ed endpoint diventati silenziosi. |
| **Historical data** | Dataset salvati, spazio occupato, la coda dei download, buchi di copertura, freschezza e download falliti. |
| **Backtest** | Migliore esecuzione salvata, la classifica, la libreria di strategie e le esecuzioni rapide preferite. |
| **Finance database** | Preferiti per classificazione e cartella, l'aggiornamento del catalogo e i preferiti obsoleti. |
| **Charts** | Layout e liste salvati, alert attivi sul grafico e attività recente degli alert. |
| **Managers' portfolios** | Gestori seguiti, i portafogli più grandi, la deriva delle posizioni e le principali posizioni per variazione. |
| **Community Docs** | Riferimenti salvati per categoria, aggiunte recenti e accesso rapido. |
| **Quant tools** | Dataset e backtest disponibili, rischio e drawdown di un singolo asset, correlazione, stagionalità, volatilità realizzata rispetto al suo intervallo storico e il regime di mercato stimato. |
| **Tax calculator** | Imposta stimata dell'anno, l'aliquota effettiva e una tabella anno su anno. |
| **Prompt store** | I tuoi prompt per tag, clicca uno per copiarlo. |
| **Resources** | Segnalibri di una categoria scelta. |
| **Agent** | Chiedi all'assistente: scegli modello e strumenti, invia, e arrivi nella conversazione. |
| **Agent activity** | Conversazioni recenti e consumo di token. |

I widget Fundamentals e Quant si aggiornano ogni cinque minuti mentre la pagina è visibile. Mantengono il risultato precedente durante l'aggiornamento e spiegano un aggiornamento fallito. Fundamentals legge solo snapshot salvati; carica o aggiorna i dati mancanti nella pagina del modulo corrispondente.

Nelle **impostazioni del widget**, scegli un dataset Quant o un basket da due a venti dataset compatibili. I membri di un basket devono condividere un timeframe; i membri intraday devono condividere anche un provider. Risk offre una confidenza VaR storica del 90%, 95% o 99%, la stagionalità offre rendimenti, volatilità, volume o range di barra, e le schede di volatilità e regime espongono la loro finestra o il numero di stati. Ogni analisi mostra la storia effettiva e la dimensione del campione.

Le impostazioni di Fundamentals ti permettono di scegliere e ordinare le serie di una bacheca macro, scegliere fino a quattro metriche aziendali o colonne di tabella, selezionare aziende peer e filtrare filing e utili sulle aziende seguite. Il TTM di un bilancio somma quattro trimestri consecutivi ed è disponibile per le voci di conto economico e di flusso di cassa; i valori dello stato patrimoniale restano osservazioni a fine periodo. Un confronto anno su anno richiede lo stesso periodo fiscale dell'anno precedente e una base di confronto positiva.

Passa il mouse, dai il focus o tocca una cella della heatmap per ispezionare il valore e il numero di campioni. Le celle mancanti sono tratteggiate, distinte da uno zero misurato. Le schede strette mostrano riepiloghi di stagionalità mensile o le correlazioni di coppia più forti. I link dei widget aprono la scheda del modulo pertinente con la serie, il dataset o il basket selezionato.

Sugli schermi fino a 767px di larghezza, le schede della dashboard si impilano in una sola colonna, e in modalità modifica i pulsanti **Sposta su** / **Sposta giù** sostituiscono il trascinamento. La disposizione salvata resta disponibile su schermi più larghi.

## Ricerca globale

La casella di ricerca nella barra in alto, a cui si dà il focus da ovunque con <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, oppure con <kbd>/</kbd> quando non stai scrivendo in un campo.

Per impostazione predefinita cerca tra i **nomi dei moduli**, le **sezioni delle Impostazioni** e le voci di **Resources**. L'**interruttore dei livelli** accanto alla casella la estende ai tuoi contenuti: pagine dell'Editor, Goals, eventi del Calendar, ToDo, Routines, promemoria, prompt, oggetti delle email, Community Docs, e società, serie e documenti di Fundamentals.

Due cose che di proposito non fa: cerca **solo in titoli e nomi, mai nei corpi**, e cerca solo nei moduli che hai installato. I risultati tornano raggruppati per tipo, prima le corrispondenze di prefisso; <kbd>↑</kbd>/<kbd>↓</kbd> e <kbd>Invio</kbd> li navigano.

## Notifiche

La campanella nella barra in alto porta un conteggio dei non letti e apre la **casella delle notifiche**, dove arrivano i promemoria di [RemindMe](/it/modules/productivity#remindme) quando scattano, insieme alle notifiche degli altri moduli: workflow dell'Automator, [webhook](/it/modules/productivity#webhooks) in entrata, job in background come i controlli della mailbox, i download di dati e le sincronizzazioni del broker, ed eventi di sicurezza come un accesso da un indirizzo mai usato da questo account. Una notifica che scatta mentre sei nell'app scivola dentro anche come banner.

La consegna a **email, Telegram, Slack o Discord** passa dai [canali di notifica](/it/config/settings#notifications) condivisi nelle Impostazioni, dove decidi anche quali moduli possono inviare a ciascuno. La casella stessa è sempre attiva e non richiede configurazione.
