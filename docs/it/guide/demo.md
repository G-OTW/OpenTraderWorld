# Modalità demo

C'è una sandbox pubblica su **[demo.opentraderworld.com](https://demo.opentraderworld.com)**: l'app reale, con dati di esempio, condivisa da tutti e ripristinata ogni **15 minuti**. Niente da installare, niente a cui registrarsi; entri già connesso come `demo`.

Questa pagina spiega cos'è quella modalità, così sai cosa stai guardando, e così puoi avviarne una tu se vuoi mostrare l'app a qualcuno.

## Cos'è

La modalità demo è una postura che il backend adotta quando parte con `OTW_DEMO=1`. **Non è mai abilitata implicitamente**: un'installazione normale non è toccata da nulla di quanto segue.

- **Il database si ripristina ogni quarto d'ora.** Il seed viene ripristinato da un template, quindi tutto ciò che cambi sparisce a :00, :15, :30 o :45. Il banner nell'app conta alla rovescia fino al prossimo.
- **Accedi automaticamente** come account `demo`. Nessuna password da indovinare, nessun account da creare.
- **Tutti condividono un solo database.** Operazioni, note e conversazioni degli altri visitatori sono visibili, e le tue sono visibili a loro. Non digitare nulla che non pubblicheresti.

## Cosa è bloccato

Il controllo è **default-deny**: una richiesta deve corrispondere a una allowlist esplicita o viene rifiutata con `demo_disabled`. Una route a cui nessuno ha pensato è chiusa, non aperta, la stessa regola che segue il [catalogo MCP](/it/config/ai-agents).

In generale:

| Bloccato | Sola lettura | Completo |
|---|---|---|
| Configurazione, logout, account e password, rete, backup, aggiornamento, cancellazione dati, installazione/scollegamento moduli, **il vault**, broker, paper trading, controllo esterno, webhook in entrata (`/api/hooks`), l'endpoint MCP stesso, canali di notifica, installazione di FinanceDatabase, download dei provider | **L'Automator**, **Bot di trading**, il modulo Webhooks, data connector, feed, file, portafogli dei manager, impostazioni e token MCP, frequenza API, dataset salvati, provider dell'agent, memorie e skill | Journal, backtest, quant, portafogli, calendario, todo, obiettivi, editor e database, prompt, risorse, abbonamenti, taxcalc, time, dashboard, ricerca, chat dell'agent |

Quindi puoi registrare un'operazione, eseguire un backtest e parlare con l'assistente; non puoi cambiare la modalità di rete, generare un token, scaricare il database, o far sì che la macchina interroghi un provider a consumo per tuo conto.

Tre di questi meritano una parola:

- **Il vault** è chiuso del tutto. È l'unico archivio il cui scopo intero è custodire credenziali, e questo database è condiviso e pubblico.
- **L'Automator** è in sola lettura: il grafo, le sue revisioni salvate e la cronologia delle esecuzioni si possono consultare, ma salvare, testare, eseguire o pianificare un flusso risponde `demo_disabled`. Un flusso raggiunge tutto ciò che il suo token concede e può chiamare qualsiasi URL, esattamente ciò che una sandbox pubblica non deve offrire.
- **Bot di trading** è in sola lettura: due bot precaricati su un conto paper demo, uno ridotto dalla sua protezione e uno messo in pausa dalla sua protezione, mostrano la pagina di un bot. Nessuno dei due è armato né gira, e creare, eseguire o armare un bot risponde `demo_disabled`.

Anche i messaggi della chat sono limitati a 2000 caratteri.

## Quote per visitatore

Gli endpoint costosi costano denaro reale, quindi ciascuno ha **due** budget: una quota per visitatore e, sopra, un tetto globale. Il solo per-IP non limiterebbe la spesa; il solo globale lascerebbe un visitatore con uno script bloccare tutti gli altri.

| | Per visitatore | Su tutta la demo | Finestra |
|---|---|---|---|
| **Esecuzioni dell'agent** | 3 | 8 | 10 minuti |
| **Esecuzioni dell'agent** | 10 | 40 | 24 ore |
| **Backtest e sweep** | 10 | 30 | 10 minuti |
| **Letture delle serie dei grafici** | 120 | 600 | 10 minuti |

L'assistente gira su una **chiave condivisa vincolata a un modello gratuito**, risolta all'avvio; la memoria a lungo termine e i server MCP esterni sono disattivati.

## Avviare la tua

La demo ha il suo stack in `deploy/demo/`, separato da un'installazione normale:

```bash
cd deploy/demo
cp .env.example .env                              # throwaway values only
docker compose -f docker-compose.demo.yml up -d --build
./reset.sh --init                                 # build the seed template once, then reset
```

Poi resettala ogni 15 minuti dal crontab dell'host, al quarto d'ora, per far coincidere il conto alla rovescia che l'app mostra:

```
*/15 * * * * /path/to/deploy/demo/reset.sh
```

Quello stack fa girare core con `OTW_DEMO=1`. Due chiavi facoltative vanno in `deploy/demo/.env`: `OTW_DEMO_LLM_KEY`, la chiave OpenRouter condivisa su cui gira l'assistente, e `OTW_DEMO_AV_KEY`, una chiave Alpha Vantage che attiva il feed di sentiment delle notizie (letta quando viene costruito il seed). Senza la prima l'assistente non può rispondere; senza la seconda quel feed resta disattivato.

Al di fuori di quello stack, gli stessi elementi sono:

```bash
OTW_DEMO=1        # in the core service's environment
otw-core --seed-demo   # once, against a scratch database
```

Il seed è pubblico: è nel repo, non contiene segreti, e termina subito se esiste già un utente `demo`. Il reset è un ripristino `CREATE DATABASE … TEMPLATE` pilotato dall'host, non dall'app.

::: warning Non puntare la modalità demo ai tuoi dati
Il seed scrive in qualunque database indichi `DATABASE_URL`, e il reset ripristina sopra di esso. Usa un database temporaneo.
:::
