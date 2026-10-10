# Panoramica dei moduli

OpenTraderWorld è un insieme di **moduli**, gruppi di funzioni che attivi singolarmente in **Impostazioni → Moduli**. Tutto è incluso nell'app; installare un modulo lo fa comparire nel selettore dei moduli (in alto a sinistra) e nella dashboard. Scollega un modulo per nasconderlo di nuovo (i suoi dati vengono mantenuti a meno che tu non li elimini).

La [dashboard, la ricerca e le notifiche](/it/modules/dashboard) stanno sopra tutti i moduli e ci sono sempre.

## Dipendenze

Alcuni moduli si appoggiano al catalogo dataset di **Historical Data** e ne richiedono l'installazione:

```
Historical Data ──▶ Historical Data Visualization
                ──▶ Backtest
                ──▶ Quant Tools
```

Tutto il resto è indipendente, anche se alcuni moduli si integrano quando sono installati entrambi (ad es. Tax Calculator può importare il PnL del Trading Journal; MyWealth può importare le holding di Portfolio Tracker; Calendar può mostrare ToDo, Goals e promemoria).

Historical Data, Visualization, Watchlists, Fundamentals e il Trading Journal condividono anche un unico elenco di **[data connector](/it/config/connectors)**: un account provider si crea una volta e si concede ai moduli che possono usarlo.

## Tutti i moduli

### Trading

| Modulo | Cosa fa |
|---|---|
| [Trading Journal](/it/modules/journal) | Registro operazioni con modelli, strutture di commissioni, FX multi-valuta e statistiche di performance. |
| [Trading Routines](/it/modules/productivity#routines) | Checklist di sessione ricorrenti: preparazione pre-market, disciplina in sessione, revisione post-market. |
| [Mindset](/it/modules/productivity#mindset) | Check-in giornalieri su umore e disciplina con andamenti. |

### Dati di mercato e analisi

| Modulo | Cosa fa |
|---|---|
| [Historical Data](/it/modules/market-data#histdata) | Scarica lo storico OHLCV da più provider in dataset locali. |
| [Historical Data Visualization](/it/modules/market-data#histviz) | Uno spazio di lavoro di grafici candele/OHLC/linea/Renko con indicatori, disegni, confronti e alert osservati dal server, live o su richiesta, su qualsiasi strumento servito da un connector. |
| [Backtest](/it/modules/market-data#backtest) | Backtester di strategie basate su regole con sizing, costi e statistiche complete. |
| [Quant Tools](/it/modules/market-data#quant) | Rischio, statistiche, volatilità e regimi di un dataset; coppie, basket e regressione fattoriale; test di overfitting su backtest salvati; sizing, calcolatori, curve dei futures e superfici di volatilità delle opzioni. |
| [Fundamentals](/it/modules/fundamentals) | Serie macro, bilanci aziendali, filing SEC, trascrizioni, ETF, un calendario di mercato e dati alternativi da fonti primarie e dagli aggregatori che colleghi. |

### Portafogli e denaro

| Modulo | Cosa fa |
|---|---|
| [Watchlists](/it/modules/portfolio#watchlists) | Watchlist di simboli con prezzi live, variazioni giornaliere, sparkline e note. |
| [Portfolio Tracker](/it/modules/portfolio#portfolios) | Valore live, registro di cassa e rendite, performance rispetto al rischio assunto, deriva dell'allocazione e stress test. |
| [MyWealth](/it/modules/portfolio#wealth) | Patrimonio netto su tutto ciò che possiedi e devi, con portafogli letti live invece che copiati. |
| [Managers' Portfolios](/it/modules/portfolio#mportfolios) | Holding 13F dei superinvestitori, consultabili e con snapshot. |
| [Tax Calculator](/it/modules/portfolio#taxcalc) | Stime fiscali per trading e investimenti da modelli per paese. |
| [Subscriptions](/it/modules/portfolio#subscriptions) | Abbonamenti ricorrenti e panoramica della spesa. |

### Notizie e ricerca

| Modulo | Cosa fa |
|---|---|
| [News](/it/modules/news-research#news) | Aggregatore di notizie RSS e API JSON con dashboard di polling. |
| [Mailbox](/it/modules/news-research#mailbox) | Newsletter, notizie di mercato e mail del broker lette dalla tua casella IMAP, senza tracker. |
| [Economic Calendar](/it/modules/news-research#economics) | Prossimi eventi macro. |
| [FinanceDatabase](/it/modules/news-research#findb) | Cerca oltre 300.000 strumenti in locale; organizza i preferiti in cartelle. |
| [Resources](/it/modules/news-research#resources) | Libreria di segnalibri per libri, link e riferimenti. |
| [Community Docs](/it/modules/news-research#community-docs) | Guide scritte dalla community, sincronizzate e leggibili offline. |

### Note e organizzazione

| Modulo | Cosa fa |
|---|---|
| [Editor](/it/modules/productivity#editor) | Editor di documenti avanzato con cartelle e database tabella/kanban/galleria. |
| [ToDo](/it/modules/productivity#todos) | Elenco attività con scadenze e categorie. |
| [Goals](/it/modules/productivity#goals) | Obiettivi con monitoraggio delle metriche e scadenze. |
| [Calendar](/it/modules/productivity#calendar) | Calendario eventi personale; sovrappone promemoria, todo e obiettivi. |
| [RemindMe](/it/modules/productivity#remindme) | Promemoria con notifiche in-app e canali email/Telegram/Slack/Discord. |
| [Time Tracker](/it/modules/productivity#time) | Timer di progetto con budget e valore a tariffa oraria. |
| [Prompt Store](/it/modules/productivity#prompt-store) | Libreria ricercabile di prompt IA riutilizzabili, con tag, valutazioni e versioni. |
| [Webhooks](/it/modules/productivity#webhooks) | URL privati in entrata che trasformano gli alert esterni in notifiche. |
| [Automator](/it/modules/automator) | Workflow sulla tua API e sul mondo esterno, a mano o pianificati. |

### IA

| Modulo | Cosa fa |
|---|---|
| [Agent](/it/modules/agent) | Assistente chat IA integrato (porta il tuo provider) che può anche agire sui tuoi dati via MCP, con memoria, skill e server MCP esterni. |
