# Installazione

Installa OpenTraderWorld sulla tua macchina o sul tuo server. Richiede circa 5 minuti.

::: info Solo containerizzato (per ora)
OpenTraderWorld gira come stack Docker Compose, l'unico deployment supportato. Un'installazione nativa è possibile ma sconsigliata: Docker mantiene l'installazione non invasiva (tutto vive in container e volumi) e veloce da ricostruire. Vedi [Installare Docker](/it/guide/docker) per il perché e per i passaggi di installazione per ogni sistema operativo.
:::

## Requisiti

- **Docker** con Docker Compose. Non ce l'hai? [Installare Docker](/it/guide/docker) copre macOS, Windows e Linux in pochi comandi.
- Linux, macOS o Windows.
- Una porta libera (**5454** per impostazione predefinita; porte **80** + **443** per le modalità HTTPS). Puoi cambiarla durante la configurazione.

Verifica che Docker sia pronto:

```bash
docker --version
docker compose version
```

## Installazione con un solo comando (consigliata)

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

L'installer verifica che Docker sia pronto, scarica i file di deploy (solo la cartella `deploy/`, nessun codice sorgente, nessuna toolchain) in `./opentraderworld`, poi passa alla configurazione guidata qui sotto, che **scarica le immagini precompilate** da Docker Hub.

Le opzioni vanno dopo `bash -s --`:

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash -s -- --dir ~/otw
```

| Opzione | Predefinito | Note |
|---|---|---|
| `--dir <path>` | `./opentraderworld` | Cartella di installazione. Rifiuta una cartella non vuota o un'installazione esistente. |
| `--ref <ref>` | `master` | Branch o tag da installare. |
| `--build` | off | Clona tutto il sorgente e compila le immagini in locale invece di scaricarle (servono `git` e la toolchain). |

## Da un clone git (alternativa)

```bash
git clone https://github.com/G-OTW/OpenTraderWorld.git
cd OpenTraderWorld/deploy
./setup.sh
```

## La configurazione guidata

Entrambi i percorsi qui sopra eseguono `deploy/setup.sh`. Pone alcune domande, genera segreti robusti, scrive la configurazione e avvia tutto.

Chiede:

| Domanda | Predefinito | Note |
|---|---|---|
| **Modalità di rete** | `1` (localhost) | `1` solo questa macchina · `2` LAN su HTTP semplice · `3` LAN su HTTPS con un certificato reale · `4` internet pubblico sul tuo dominio. Modificabile in seguito, vedi [Rete e accesso remoto](/it/config/network). |
| **Nome utente admin** | `admin` | L'account admin viene creato per te; viene generata una password robusta e mostrata **una sola volta**. |
| **Porta HTTP** | `5454` | Solo modalità 1-2; le modalità 3 e 4 usano 80 + 443. |
| **Dominio DuckDNS / token / IP LAN** | nessuno | Solo modalità 3. |
| **Dominio pubblico** | nessuno | Solo modalità 4, e deve già risolvere verso questo server. |
| **Livello di log** | `info` | `trace` / `debug` / `info` / `warn` / `error`. |

I **segreti** del database e delle sessioni sono **generati automaticamente**, quindi non li digiti mai. Vengono scritti in `deploy/.env` (permessi del file `600`, mai committato su git).

Alla fine lascia che lo script avvii lo stack: attende l'API, **crea il tuo account admin** e stampa la password generata **una sola volta**, quindi copiala prima di chiudere il terminale.

Per impostazione predefinita la configurazione **scarica le immagini precompilate** da Docker Hub: nessuna toolchain Rust o Node, primo avvio in un paio di minuti. Esegui `./setup.sh --build` per compilare invece i tre servizi dai sorgenti (sviluppo, modifiche locali).

::: tip Server headless
L'admin viene creato dal core stesso al primo avvio (da `deploy/.env`), quindi non serve un browser sul server. Annota la password stampata e accedi da qualsiasi macchina che raggiunga l'app.
:::

::: warning Reinstallare sopra dati precedenti
Se esistono volumi Docker di un'installazione precedente, la configurazione offre di eliminarli. Il valore predefinito è **No** ovunque: eliminare (e perdere il database precedente) avviene solo con un esplicito `y`. Segreti nuovi su un volume di database vecchio non possono funzionare, quindi rifiutare interrompe la configurazione invece di avviare uno stack rotto.
:::

## Installazione manuale (alternativa)

Se preferisci configurare a mano:

```bash
cd deploy
cp .env.example .env
```

Modifica `.env` e imposta almeno:

- `POSTGRES_PASSWORD`: una password robusta
- `DATABASE_URL`: deve contenere la stessa password, ad esempio `postgres://otw:YOUR_PASSWORD@postgres:5432/opentraderworld`
- `SESSION_SECRET`: una lunga stringa casuale

Poi avvia lo stack. Per impostazione predefinita **scarica le immagini precompilate** da Docker Hub (nessuna toolchain Rust o Node richiesta):

```bash
docker compose -f docker-compose.yml -f docker-compose.images.yml \
  --env-file .env --env-file network.env up -d
```

::: details Compilare dai sorgenti
Per lo sviluppo, o per eseguire modifiche locali, ometti l'override delle immagini e compila tu i tre servizi (serve la toolchain; la build Rust è lenta):

```bash
docker compose --env-file .env --env-file network.env up --build -d
```
:::

## Creare l'admin (solo installazioni manuali)

Se hai usato `./setup.sh` e hai lasciato che avviasse lo stack, **il tuo admin esiste già**, quindi vai avanti.

Altrimenti apri l'app nel browser (`http://localhost:5454` per impostazione predefinita). Alla prima visita OpenTraderWorld rileva che non c'è ancora un admin e mostra la **procedura guidata di configurazione**: scegli nome utente e password (minimo 8 caratteri), invia, e arrivi sulla dashboard. Le password sono salvate con hash argon2.

::: details Creare l'admin dalla CLI (headless, senza browser)
Chiama l'endpoint di primo avvio dall'interno dello stack: funziona a prescindere dall'interfaccia di bind o dalla modalità TLS, e rifiuta (HTTP 409) se esiste già un admin:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T caddy \
  wget -qO- --header=Content-Type:application/json \
  --post-data='{"username":"admin","password":"CHOOSE-A-STRONG-ONE"}' \
  http://core:8080/api/setup
```
:::

## Verificare che giri

- App: `http://localhost:5454` (o la porta/dominio che hai scelto)
- Health check: `http://localhost:5454/api/health` → `{"status":"ok","service":"otw-core",...}`

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## Operazioni quotidiane

Eseguile da `deploy/`:

| Azione | Comando |
|---|---|
| Avviare | `docker compose up -d` |
| Fermare | `docker compose down` |
| Vedere i log | `docker compose logs -f` |
| Scaricare immagini più recenti | `docker compose -f docker-compose.yml -f docker-compose.images.yml pull && docker compose up -d` |
| Ricompilare dopo modifiche al codice (build da sorgenti) | `docker compose up --build -d` |
| Fermare **ed eliminare tutti i dati** | `docker compose down -v` |

I tuoi dati vivono nei volumi con nome di Docker e **persistono** tra `up`/`down`. Vengono eliminati solo con `down -v`.

## Prossimi passi

- [Primi passi](/it/guide/first-steps): accedi, imposta i valori predefiniti, installa i moduli.
- [Rete e accesso remoto](/it/config/network): raggiungi l'app da altri dispositivi, HTTPS in LAN, esposizione pubblica.
- Qualcosa non va? Vedi [Risoluzione dei problemi](/it/guide/troubleshooting).
