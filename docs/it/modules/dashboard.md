# Dashboard e navigazione

La schermata iniziale dell'app, più le due cose che stanno sopra ogni modulo: la casella di ricerca e la casella delle notifiche.

## Pagine della dashboard

La dashboard si apre su una pagina integrata **Moduli**: una tessera per modulo installato, ricostruita automaticamente quando installi e scolleghi. Non viene mai modificata né eliminata; riflette semplicemente ciò che hai.

Sopra a questa crei **le tue pagine**. Ognuna ha un nome, una descrizione opzionale e un breve **tag** mostrato sul suo chip. Una pagina è la **predefinita**: quella su cui si apre la dashboard, e il cui chip viene ordinato per primo.

Usale come si divide una giornata di trading: una pagina *Morning* con il feed di notizie, il calendario economico e la checklist della routine; una pagina *Positions* con il portafoglio e la watchlist; una pagina *Admin* con todo e timer.

## Modificare un layout

**Modifica layout** trasforma una pagina in una griglia di righe su 12 colonne. In modalità modifica puoi:

- **aggiungere righe** e inserirvi **tessere di modulo** (un link a un modulo, e lo stesso modulo può comparire in qualsiasi numero di pagine) o **widget**;
- **ridimensionare** qualsiasi tessera per estensione in colonne, e trascinare le tessere tra le righe;
- impostare il **preset di altezza** di un widget (compatto, standard o alto) e aprirne la **configurazione** (l'ingranaggio sulla tessera);
- inserire **righe distanziatrici** per far respirare i blocchi.

Le tessere sono link, non copie: rimuoverne una da una pagina non tocca mai il modulo né i suoi dati.

## Widget

Un widget è un'anteprima live e interattiva di un modulo: legge e scrive tramite l'API di quel modulo, quindi ciò che fai nel widget è reale. I widget il cui modulo non è installato semplicemente non vengono offerti.

| Widget | Cosa fa |
|---|---|
| **Free text** | Una nota o un titolo che scrivi tu, in markdown-lite. |
| **News feed** | Ultimi elementi di un feed scelto, come elenco o griglia. |
| **Mailbox** | L'ultima posta non letta, dalla più recente. |
| **Time tracker** | Avvia/ferma un timer di progetto senza lasciare la pagina. |
| **Quick trade** | Scegli una categoria + un modello e apri il modulo di aggiunta operazione. |
| **Goals** | Un breve elenco di obiettivi con avanzamento; aggiungine uno inline. |
| **ToDo** | Attività aperte, spuntabili sul posto. |
| **Trading routine** | La checklist di oggi, spuntabile sul posto. |
| **Mindset** | Il check-in del giorno. |
| **Reminder** | Un modulo rapido per aggiungere un promemoria. |
| **Calendar** | Oggi e questa settimana a colpo d'occhio. |
| **Economic calendar** | Prossimi eventi macro, in versione compatta. |
| **Portfolio** | Un riepilogo del portafoglio con valore live. |
| **Subscriptions** | La spesa ricorrente mensile, poi cosa si rinnova per prossimo. |
| **Net worth** | Patrimonio netto attuale, la sua variazione su una finestra che imposti e una sparkline. |
| **Watchlist** | Quotazioni live di una lista scelta: prezzo, variazione 24h e 7g. |
| **Fundamentals** | Serie e bacheche macro, uno snapshot aziendale, una voce di bilancio per trimestre, anno o TTM, aziende ordinabili, filing filtrati, prossimi utili e valutazione rispetto ai peer salvati. Letto dai dati salvati senza consumare quota del provider. |
| **Quant** | Dataset e backtest disponibili, rischio e drawdown di un singolo asset, correlazione, stagionalità, volatilità realizzata rispetto al suo intervallo storico e il regime di mercato stimato. |
| **Prompt store** | I tuoi prompt per tag, clicca uno per copiarlo. |
| **Resources** | Segnalibri di una categoria scelta. |
| **Agent** | Chiedi all'assistente: scegli modello e strumenti, invia, e arrivi nella conversazione. |

I widget Fundamentals e Quant si aggiornano ogni cinque minuti mentre la pagina è visibile. Mantengono il risultato precedente durante l'aggiornamento e spiegano un aggiornamento fallito. Fundamentals legge solo snapshot salvati; carica o aggiorna i dati mancanti nella pagina del modulo corrispondente.

Nelle **impostazioni del widget**, scegli un dataset Quant o un basket da due a venti dataset compatibili. I membri di un basket devono condividere un timeframe; i membri intraday devono condividere anche un provider. Risk offre una confidenza VaR storica del 90%, 95% o 99%, la stagionalità offre rendimenti, volatilità, volume o range di barra, e le schede di volatilità e regime espongono la loro finestra o il numero di stati. Ogni analisi mostra la storia effettiva e la dimensione del campione.

Le impostazioni di Fundamentals ti permettono di scegliere e ordinare le serie di una bacheca macro, scegliere fino a quattro metriche aziendali o colonne di tabella, selezionare aziende peer e filtrare filing e utili sulle aziende seguite. Il TTM di un bilancio somma quattro trimestri consecutivi ed è disponibile per le voci di conto economico e di flusso di cassa; i valori dello stato patrimoniale restano osservazioni a fine periodo. Un confronto anno su anno richiede lo stesso periodo fiscale dell'anno precedente e una base di confronto positiva.

Passa il mouse, dai il focus o tocca una cella della heatmap per ispezionare il valore e il numero di campioni. Le celle mancanti sono tratteggiate, distinte da uno zero misurato. Le schede strette mostrano riepiloghi di stagionalità mensile o le correlazioni di coppia più forti. I link dei widget aprono la scheda del modulo pertinente con la serie, il dataset o il basket selezionato.

Sugli schermi di telefono fino a 480px di larghezza, le schede della dashboard si impilano in una sola colonna. La disposizione salvata resta disponibile su schermi più larghi e nell'editor del layout.

## Ricerca globale

La casella di ricerca nella barra in alto, a cui si dà il focus da ovunque con <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, oppure con <kbd>/</kbd> quando non stai scrivendo in un campo.

Per impostazione predefinita cerca tra i **nomi dei moduli**, le **sezioni delle Impostazioni** e le voci di **Resources**. L'**interruttore dei livelli** accanto alla casella la estende ai tuoi contenuti: pagine dell'Editor, Goals, eventi del Calendar, ToDo, Routines, promemoria, prompt e Community Docs.

Due cose che di proposito non fa: cerca **solo in titoli e nomi, mai nei corpi**, e cerca solo nei moduli che hai installato. I risultati tornano raggruppati per tipo, prima le corrispondenze di prefisso; <kbd>↑</kbd>/<kbd>↓</kbd> e <kbd>Invio</kbd> li navigano.

## Notifiche

La campanella nella barra in alto porta un conteggio dei non letti e apre la **casella delle notifiche**, dove arrivano i promemoria di [RemindMe](/it/modules/productivity#remindme) quando scattano, insieme a tutto ciò che un [webhook](/it/modules/productivity#webhooks) in entrata reindirizza lì. Una notifica che scatta mentre sei nell'app scivola dentro anche come banner.

La consegna a **email, Telegram, Slack o Discord** passa dai [canali di notifica](/it/config/settings#notifications) condivisi nelle Impostazioni, dove decidi anche quali moduli possono inviare a ciascuno. La casella stessa è sempre attiva e non richiede configurazione.
