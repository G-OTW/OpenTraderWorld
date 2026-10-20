# Portafogli e patrimonio

Moduli indipendenti per tenere traccia di ciò che osservi, ciò che possiedi, quanto ti costa e quanto potrebbe essere il conto fiscale.

## Watchlists {#watchlists}

Liste con nome di simboli che vuoi tenere d'occhio: nessuna posizione, nessun registro, solo quotazioni.

- **Aggiungi simboli** cercando (crypto via CoinGecko, azioni/ETF via Yahoo), parti da un **modello curato** (Crypto Top 10, Magnificent 7, ETF sugli indici USA, Semiconduttori), oppure **importa un portafoglio di Portfolio Tracker**, dove reimportare riconcilia invece di duplicare.
- Ogni riga mostra il prezzo live in USD, le **variazioni 24h / 3g / 7g / 30g**, una **sparkline a 30 giorni**, la borsa e una **nota** libera per simbolo. Ordina per qualsiasi colonna, filtra per nome.
- **Aggiornamento automatico** per lista, da ogni minuto a giornaliero (15 min predefinito). La pagina stima la frequenza delle richieste e **avvisa prima che un intervallo rischi il throttling delle API gratuite**. Le quotazioni sono in cache lato server, quindi riaprire la pagina è istantaneo e non colpisce mai i provider.

### Fonti di quotazione personalizzate

Le fonti pubbliche (CoinGecko / Yahoo) funzionano subito senza configurazione. Se hai un tuo account di dati di mercato, collega un **[data connector](/it/config/connectors)**, lo stesso account provider condiviso che usano [Historical Data](/it/modules/market-data#histdata) e il grafico, creato una volta e concesso a Watchlists. Ogni connector porta le proprie credenziali (digitate, o scelte dal [Vault](/it/config/settings#vault)) e il proprio limite di richieste.

- Una lista può **fissare un connector come fonte predefinita**, e ogni simbolo può sostituirla: *segui la lista*, *auto*, o un connector specifico.
- I **ticker del provider** per simbolo (`BTCUSDT`, `AAPL.US`, …) sono derivati automaticamente e restano modificabili quando un provider nomina un simbolo in modo diverso.
- Una quotazione che fallisce emerge **sulla propria riga**, così un solo simbolo cattivo non nasconde il resto della lista.

::: warning Conosci i limiti del tuo piano
Una lista basata su una fonte personalizzata sblocca intervalli di aggiornamento da **5s-30s**. Sono abbastanza rapidi da bruciare in fretta un piano API: le chiamate in eccesso falliscono e possono far bloccare la tua chiave. Tieni d'occhio i contatori in **Impostazioni → Frequenza API**.
:::

### Alert di prezzo

Ogni simbolo può portare alert, impostati dalla campanella sulla sua riga. Ognuno si legge come una frase che componi da sinistra a destra, *avvisami quando BTC si muove ±5% da adesso*:

- **Un livello** (prezzo sopra o sotto un valore), oppure **un movimento** misurato in **%** o in **$**, su, giù o in entrambe le direzioni.
- Un movimento è misurato **da adesso**, o su una **finestra mobile** (1h, 4h, 12h, 1g, 3g, 7g, 30g).
- **Una volta sola o ripetuto**, con un ritardo di riarmo (da 5m a 1g) così una sola oscillazione non può scattare a ogni aggiornamento.
- **Destinazioni**: la casella in-app più qualsiasi [canale di notifica](/it/config/settings#notifications) scelto per alert. Watchlists può puntare solo ai canali che le sono stati concessi.

Gli alert sono valutati **lato server dentro il ciclo di aggiornamento**, quindi scattano con la pagina chiusa e il browser spento.

### Descrizione della lista

Una watchlist porta una descrizione modificabile sotto il suo nome, per a cosa serve davvero la lista.

## Portfolio Tracker {#portfolios}

Valore live delle tue holding reali, un portafoglio per account o tema.

- **Aggiungi asset** cercando (coin crypto o azioni/ETF) e registra **operazioni di acquisto/vendita** (data, quantità, prezzo, fee, nota). P/L realizzato e non realizzato, costo medio e pesi sono calcolati dal registro.
- **Valuta di trading per asset**: ogni asset dichiara la valuta in cui sono inserite le sue operazioni, così un'azione comprata in EUR non viene registrata come se fosse in USD. Le etichette del modulo seguono quella valuta. Le quotazioni spot restano in USD: costo base e P/L realizzato convertono **alla data di ogni operazione** usando i cambi FX del [Trading Journal](/it/modules/journal), quindi un acquisto di tre anni fa mantiene il suo cambio storico. Un popover nel modulo spiega da dove viene ogni prezzo.
- **Aggiornamento automatico**: un passaggio di riconciliazione una tantum verifica ogni holding rispetto alla sua fonte di prezzo; correggi quelle che risultano *unresolved* (o segnale come manuali) e abilita l'**aggiornamento automatico giornaliero**, dopo il quale i prezzi si aggiornano in background ogni giorno.
- Per portafoglio: valore, costo base, P/L non realizzato/realizzato/totale, miglior e peggior asset, **allocazione** per asset o classe, e un **grafico del valore nel tempo** (giorno/settimana/mese/anno) che si riempie man mano che si accumulano gli aggiornamenti.
- La **descrizione** resta modificabile dopo la creazione, accanto a una nota ripiegabile della **tesi di investimento** conservata con il portafoglio, sul perché detieni ciò che detieni.

### Cassa, rendite e costi

Il registro non è solo acquisti e vendite. **Cassa e rendite** nella scheda operazioni registra un
**deposito**, un **prelievo**, un **dividendo**, un **interesse**, una **cedola**, una **fee** o una
**tassa**, ciascuno nella propria valuta e con una fee trattenuta opzionale. La rendita può nominare
l'holding che l'ha pagata, o nulla se è arrivata dall'account stesso.

Da quelle righe il portafoglio ottiene un saldo di cassa per valuta, una rendita totale e un vero
patrimonio netto (posizioni più cassa). La cassa negativa viene mostrata, mai troncata: significa margine, o un
registro a cui mancano i depositi, ed entrambi valgono la pena di essere visti.

### Analisi

La scheda **Analisi** risponde a performance, rischio ed esposizione in un solo posto. Sette viste
indipendenti, ciascuna richiede solo i dati che le servono, quindi *Book* si mostra istantaneamente su un'installazione nuova
mentre *Stress* richiede le candele.

Una vista che non può rispondere **dice perché e cosa fare**. Non mostra mai uno zero che non
ha misurato. Nessuna storia ancora, un book troppo corto per annualizzare, nessun benchmark scelto, nessuna candela
per esso, nessun target impostato: ciascuno è una frase e un pulsante, non un grafico vuoto.

| Vista | Richiede | Risponde |
|---|---|---|
| **Book** | il registro, nient'altro | patrimonio netto, investito rispetto alla cassa, non realizzato, realizzato, rendite, allocazione per classe |
| **Performance** | storia giornaliera | rendimento, IRR, annualizzato, contributi netti, per finestra |
| **Risk** | storia giornaliera | volatilità, drawdown, Sharpe, Sortino, Calmar, periodi migliori e peggiori |
| **Benchmark** | storia giornaliera e le candele del benchmark | cosa avrebbe reso l'indice alla tua volatilità, alpha, beta, capture |
| **Income & costs** | le righe di cassa del registro | rendite incassate, costi pagati, drag annuale, la curva senza fee |
| **Allocation** | un'allocazione target | attuale contro target, deriva, le operazioni che la chiudono |
| **Stress** | candele giornaliere per holding | replay storico e shock fattoriali, con la quota coperta |

Ogni vista funziona con lo stesso selettore di finestra: 1M, 3M, 6M, YTD, 1Y, 3Y, 5Y, tutto, o un intervallo
personalizzato. Una finestra più lunga della tua storia è riportata come **non coperta**, con i giorni che
contiene davvero, invece di spacciarla per tre anni pieni.

### Performance e rischio

**Performance** riporta un rendimento time-weighted accanto a un IRR, e rispondono a domande
diverse. Il time-weighted è ciò che hanno fatto gli investimenti, corretto per i depositi, perché un contributo
non è un rally. L'IRR è ciò che **hai** ottenuto tu, money-weighted, quindi comprare bene al momento giusto si vede
lì e da nessun'altra parte. I contributi netti stanno accanto, e un rendimento sotto i due mesi non viene
annualizzato: moltiplicare sei settimane per otto è una previsione, non una misura.

**Risk** legge la stessa curva: volatilità, max drawdown, Sharpe, Sortino, Calmar, la quota di
giorni chiusi in rialzo, miglior e peggior giorno, mese, trimestre e anno, e ogni drawdown più profondo
del 2 % con **quanto tempo ha impiegato a riempirsi**. Uno ancora aperto è segnato come in corso, con quanto sei
sotto l'ultimo massimo e quanti giorni sono passati.

Il fattore di annualizzazione è **misurato dalla tua curva**, non presunto. Un book azionario scambia
circa 252 giorni all'anno e uno crypto 365, e uno misto non è né l'uno né l'altro. Sharpe e
Sortino usano il tasso risk-free impostato nelle impostazioni di misura.

### Benchmark

Scegli uno strumento di cui hai le candele giornaliere (SPY, QQQ, BTCUSDT) e la pagina risponde all'unica
domanda che chiude una discussione: **cosa avrebbe reso quell'indice alla tua volatilità**, accanto a
ciò che hai effettivamente fatto. Battere l'indice prendendo tre volte il suo rischio non è
batterlo.

Sotto: rendimento totale e annualizzato per entrambi, volatilità, max drawdown e Sharpe affiancati,
poi alpha, beta, tracking error, information ratio e up/down capture.

Il tuo book è misurato sulle **sessioni del benchmark**. Confronta un portafoglio 24/7 con un
indice giorno per giorno e ogni lunedì dell'indice si mangia un tuo weekend, il che sottostima
silenziosamente il tuo rendimento.

### Rendite e costi

Cosa hai incassato, cosa hai pagato e quanto ti è costato pagare. Dividendi, interessi e
cedole da una parte; fee di trading e fee di account dall'altra, con il drag annuale come
quota del tuo patrimonio netto medio.

La curva è disegnata due volte: com'è andata, e lo stesso book senza le gambe delle fee. Le fee
sono già dentro il tuo costo base e la tua cassa, quindi questo è un confronto, non una sottrazione
che potresti fare da solo.

### Allocazione target

Di' quale quota del patrimonio netto ogni bucket dovrebbe detenere e quanto può derivare prima che conti
come fuori, in **Imposta target**. Un'allocazione deve sommare al 100 %, e a un bucket si può dare
il resto con un clic. Salvare un elenco vuoto spegne la vista.

La vista poi mostra attuale contro target per bucket, la deviazione, se ciascuno è nella
sua banda, e le **operazioni che chiuderebbero il divario**: compra tanto di quello, vendi tanto di
questo. Tutto ciò che detieni senza target è elencato invece che ignorato.

Solo visualizzazione. Nulla qui inserisce un ordine, e nulla ribilancia da solo.

### Stress test

Due motori, ed entrambi ti dicono quanta parte del tuo book il numero copre.

Il **replay storico** applica il percorso giornaliero realizzato del 2008, 2020, 2022, del Q4 2018 o del
picco crypto 2021 a ciò che detieni oggi, usando le candele degli strumenti in quelle date. Nessun
modello, nessun proxy. Uno strumento che allora non esisteva non ha percorso: viene **nominato ed
escluso**, mai sostituito con un indice.

Lo **shock fattoriale** muove uno strumento reale (S&P 500, Nasdaq, tassi, EUR/USD, petrolio, credit
spread) e raggiunge ogni holding tramite una sensibilità **misurata**, stimata sulle sue stesse
candele. Una holding senza candele, con storia troppo corta o con un fit senza potere esplicativo
non ottiene **nessun beta**: finisce in *unexplained* con il suo peso, e il titolo recita
"−11,8 % sul 74 % del book che si è potuto misurare". La cassa ha beta zero, che è
di solito l'unica diversificazione già presente.

Uno shock di tassi in punti base raggiunge un'obbligazione tramite una duration scritta nello scenario, così la
cifra può essere discussa. Recessione, picco di inflazione e allargamento del credito sono forniti come combinazioni modificabili
di quelle gambe.

Un pannello di prontezza elenca cosa può essere stressato e cosa no prima che tu esegua qualsiasi cosa, così
un risultato scarno è spiegato in anticipo e non dopo.

### Storia giornaliera

Ogni misura sopra tranne *Book* richiede una curva, e gli snapshot iniziano solo il giorno in cui attivi
il job giornaliero. Un portafoglio che tieni da sei anni sarebbe altrimenti misurato da
martedì scorso. Quindi la curva è **ricostruita dal registro e dalle candele salvate**, giorno per giorno.

L'ingranaggio nella scheda Analisi apre **Configurazione della misura**:

1. **Nomina il ticker delle candele di ogni asset** e la valuta in cui sono quotate. Un ticker
   nel tuo registro non è sempre il simbolo che serve il tuo provider, e un'azione comprata in EUR
   valutata con candele in USD è sbagliata del tasso di cambio.
2. **Scarica le candele mancanti**. Sono messe in coda come normali job di [Historical Data](/it/modules/market-data#histdata)
   tramite i connector concessi ai portafogli. Se nessuno di essi porta uno dei tuoi
   strumenti, viene nominato, con cosa concedere.
3. **Ricostruisci la curva**. Riporta i giorni ricostruiti, e i giorni saltati perché una holding
   non aveva candela quel giorno. Un giorno che non si può valutare non viene salvato, invece di essere salvato sbagliato.

Modificare un'operazione con data nel passato segna la curva come **obsoleta da quella data** e lo dice.
Ricostruire resta una tua scelta. Aggiornare un portafoglio scarica anche ciò che manca ed estende
la curva di conseguenza, e ti avvisa quando un broker non può servire uno dei tuoi strumenti.

### Importare un registro di operazioni

**Importa** nell'intestazione del portafoglio legge un export del broker, un foglio di calcolo o un altro tracker (CSV, TSV, JSON, Parquet) e trasforma ogni riga in una operazione di acquisto o vendita. Stesso motore di rilevamento dell'[import del journal](/it/modules/journal#import-a-trade-book): intestazioni in sei lingue più sniffing dei valori, convenzioni di delimitatore, decimali e date decise per colonna, colonne non identificate lasciate non mappate.

Tre cose che rifiuta di indovinare:

- **Cos'è un simbolo.** Ogni simbolo nel file deve puntare a un asset: uno che già detieni in questo portafoglio (abbinato automaticamente), un nuovo asset da creare, o *salta*. Un simbolo non risolto blocca l'import, e gli asset sono creati solo dopo la tua conferma.
- **Una riga che non è né acquisto né vendita né un tipo che riconosce** è elencata come errore di riga invece di essere inventata come operazione. Dividendi, depositi, prelievi, fee e tasse **sono** riconosciuti, in sei lingue, e si registrano come tali. Se il file non dichiara alcun tipo, imposta il predefinito una volta per tutto l'import.
- **Un prezzo mancante** è derivato da importo ÷ quantità e segnalato, mai riempito in silenzio.

Nulla viene scritto finché non convalidi l'anteprima. Ogni import è un **batch**, annullabile per intero (gli asset creati restano), e deduplicato **per portafoglio**, quindi reimportare lo stesso file non cambia nulla.

### Importare holding da un broker

**Da un broker** nell'intestazione del portafoglio legge il bilancio di un [conto broker](/it/config/brokers) invece di un file: la differenza rispetto al tuo registro è mostrata riga per riga, e ogni riga che accetti scrive l'unica operazione che fa concordare il portafoglio. Il costo base è usato dove il broker ne pubblica uno (Interactive Brokers) e chiesto dove non lo fa (gli exchange crypto pubblicano una quantità e nient'altro).

## MyWealth {#wealth}

Patrimonio netto su **tutto**: conti di intermediazione, immobili, crypto, contanti, beni di valore. Dove Portfolio Tracker segue holding con prezzo live, MyWealth traccia qualsiasi asset che valuti tu.

- Aggiungi asset con nome, tipo, valuta e categoria, poi **registra aggiornamenti di valore** nel tempo (prezzo × quantità, o un valore diretto, con una nota). La storia è modificabile.
- **Grafico del patrimonio netto** per mese o anno, più una ripartizione per categoria. Multi-valuta con la stessa gestione FX del journal (gli asset senza cambio sono esclusi e segnalati).
- **Modelli**, come quelli del journal: i campi riservati prezzo/quantità alimentano il valore, i campi personalizzati contengono note per revisione.
- **Posseduto o dovuto**: un asset può essere una **passività** (un mutuo, un prestito), così il titolo è un vero patrimonio netto. La pagina mostra ciò che possiedi e ciò che devi prima di compensarli.
- **Collega un portafoglio** invece di copiarlo: un portafoglio collegato è letto live dal tracker ogni volta, così il suo valore nel tuo patrimonio netto non è mai una copia obsoleta.
- **Età della valutazione**: di' a un asset ogni quanto va rivalutato e la pagina nomina quelli invecchiati oltre, il più vecchio per primo. Una casa valutata tre anni fa è sbagliata in silenzio, e questo rompe il silenzio. Un portafoglio collegato non diventa mai obsoleto, viene letto, non ricordato.

## Managers' Portfolios {#mportfolios}

Sfoglia i **portafogli 13F dei superinvestitori**: cosa detengono i famosi gestori di fondi, dimensioni delle posizioni, attività recente, valore riportato vs attuale, range a 52 settimane. Filtra per gestore o per ticker (*chi detiene AAPL?*).

Dato che i dati 13F cambiano ogni trimestre, puoi **salvare snapshot** di qualsiasi portafoglio e confrontare nel tempo.

## Tax Calculator {#taxcalc}

Stima approssimativa delle tasse su trading e investimenti. **Non è consulenza fiscale.**

- I **profili** partono da **modelli per paese** (individuale o professionale) e restano completamente modificabili: aliquota marginale sul reddito, oneri sociali, franchigie su plusvalenze e dividendi, **scaglioni di imposta patrimoniale** opzionali (ad es. CH, ES, NO), scaglioni di sgravio a lungo termine.
- Inserisci le cifre in modalità **Summary** (valore iniziale/finale, contributi, prelievi, quota realizzata) o **Itemized** (plusvalenze, guadagni su derivati, guadagni crypto, dividendi, interessi, perdite pregresse riportate).
- **Carica Trading Journal**: con il journal installato, un clic carica il PnL realizzato di un anno fiscale, ripartito in guadagni capitale / derivati / crypto, convertito al cambio di fine anno.
- **Da un broker**: con un [conto broker](/it/config/brokers) concesso al tax calculator, un anno fiscale viene letto direttamente dall'account, integrato in posizioni chiuse e totalizzato per riga del modulo, con ogni dismissione convertita al cambio della propria data di uscita.
- I risultati mostrano l'imposta stimata con una ripartizione per voce (imponibile, franchigia, base, aliquota) e l'aliquota effettiva. Salva gli scenari nella cronologia per confrontare.

## Subscriptions {#subscriptions}

Ogni costo ricorrente in un elenco (strumenti di trading, feed di dati, streaming) con prezzo, valuta, frequenza di fatturazione (settimanale/mensile/trimestrale/annuale) e categoria.

Ottieni **grafici di spesa** mensili/annuali (raggruppati o per abbonamento), l'**equivalente mensile** di ciascun abbonamento, le prossime date di fatturazione e i totali per il mese prossimo. Metti in pausa un abbonamento per tenerlo in elenco senza contarlo.
