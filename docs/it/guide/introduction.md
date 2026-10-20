# Cos'è OpenTraderWorld?

> Sito del progetto: **[opentraderworld.com](https://opentraderworld.com)**: tour dei moduli,
> [demo live](https://demo.opentraderworld.com), [guide della community](https://opentraderworld.com/docs)
> e [voto sulla roadmap](https://opentraderworld.com/suggestions).

OpenTraderWorld è una **piattaforma web self-hosted per trader e investitori**. La installi una volta con Docker sul tuo computer o server, la apri in un browser e ottieni uno spazio privato fatto di moduli: un journal di trading, dati storici di mercato con grafici e backtest, monitoraggio di portafogli e patrimonio netto, un aggregatore di notizie, note, checklist e altro.

**Gratuita per tutti, uso personale o professionale. Source-available (FSL-1.1-MIT).** L'unica cosa che non puoi fare è rivenderla o offrirla come servizio a pagamento. Il principio guida: *diventa redditizio prima di spendere un centesimo.*

## Perché self-hosted?

- **I tuoi dati restano tuoi.** Operazioni, portafogli, note e voci del journal vivono in un database PostgreSQL sulla tua macchina, non sul server di qualcun altro.
- **Privata per impostazione predefinita.** Dopo l'installazione l'app ascolta solo su `localhost`. Esporla alla tua LAN o a internet è una scelta esplicita che fai in [Impostazioni → Rete](/it/config/network).
- **Nessun abbonamento.** Gli strumenti di base non costano nulla da far girare. Alcuni moduli possono usare opzionalmente provider di dati esterni (molti con piani gratuiti), e le chiavi API le porti tu.

## Come funziona

Uno stack `docker compose`, quattro servizi:

| Servizio | Ruolo |
|---|---|
| **core** | Server API in Rust (Axum): tutta la logica applicativa, scheduler, job in background |
| **postgres** | PostgreSQL: l'unico posto in cui vivono i tuoi dati |
| **frontend** | App SvelteKit a pagina singola, compilata una volta al momento del deploy |
| **caddy** | Reverse proxy: serve l'app, inoltra `/api`, gestisce i certificati HTTPS |

L'app è **single-user**: un solo account admin, creato all'installazione. Non c'è modalità multi-tenant, condivisione o gestione utenti da configurare.

Docker è attualmente l'**unico deployment supportato**: mantiene l'installazione non invasiva e veloce da ricostruire ([perché, e come ottenere Docker](/it/guide/docker)). Un'installazione nativa è possibile ma sconsigliata.

## I moduli

I moduli sono insiemi di funzioni che installi o scolleghi da **Impostazioni → Moduli**. Tutto è incluso nell'app, e installare un modulo significa solo attivarlo. In evidenza:

- **[Trading Journal](/it/modules/journal)**: registra le operazioni con modelli, strutture di commissioni, PnL multi-valuta e statistiche di performance complete.
- **[Dati di mercato e backtest](/it/modules/market-data)**: scarica lo storico OHLCV da più provider, visualizza qualsiasi strumento live o su richiesta con indicatori, esegui il backtest di strategie basate su regole e fai analisi quant su dataset, backtest salvati, curve dei futures e catene di opzioni.
- **[Portafogli e patrimonio](/it/modules/portfolio)**: monitoraggio live dei portafogli, storico del patrimonio netto, holding 13F dei superinvestitori, stime fiscali.
- **[Notizie e ricerca](/it/modules/news-research)**: dashboard di notizie RSS/API, calendario economico, un catalogo di ricerca da 300k strumenti.
- **[Note e organizzazione](/it/modules/productivity)**: editor di testo ricco con database, todo, obiettivi, calendario, promemoria, routine di trading e check-in sul mindset.
- **[Agent IA](/it/modules/agent)**: assistente chat integrato (porta il tuo provider) che può agire sui tuoi dati via MCP, con memoria, skill e server MCP esterni.

Vedi l'[elenco completo dei moduli](/it/modules/).

## Prossimi passi

1. [Installa OpenTraderWorld](/it/guide/install): circa 5 minuti con Docker.
2. [Muovi i primi passi](/it/guide/first-steps): accedi, scegli i valori predefiniti, installa i moduli.
3. [Configura l'accesso di rete](/it/config/network): se vuoi raggiungerlo da altri dispositivi.

Non sei pronto a installare? Prova la [demo live](https://demo.opentraderworld.com), un'istanza condivisa
con dati di esempio, ripristinata ogni 15 minuti. [Cos'è la modalità demo](/it/guide/demo) e cosa blocca.
