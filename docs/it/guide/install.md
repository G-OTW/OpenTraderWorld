# Installazione

Installa OpenTraderWorld sulla tua macchina o sul tuo server. Richiede circa 5 minuti.

::: info Solo containerizzato (per ora)
OpenTraderWorld gira come stack Docker Compose, l'unico deployment supportato. Un'installazione nativa è possibile ma sconsigliata: Docker mantiene l'installazione non invasiva (tutto vive in container e volumi) e veloce da ricostruire. Vedi [Installare Docker](/it/guide/docker) per il perché e per i passaggi di installazione per ogni sistema operativo.
:::

## Requisiti

- **Docker** con Docker Compose. Non ce l'hai? [Installare Docker](/it/guide/docker) copre macOS, Windows e Linux in pochi comandi.
- Linux, macOS o Windows.
- Una porta libera (**5454** per impostazione predefinita; porte **80** + **443** per le modalità HTTPS). Puoi cambiarla durante la configurazione. Le modalità localhost e LAN su HTTP semplice occupano anche la **8443** su `127.0.0.1`.

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
| `--yes` | off | Passato a `setup.sh`: prende ogni valore predefinito quando non c'è un terminale da cui chiedere. |
| `--wipe-volumes` | off | Passato a `setup.sh`: rimuove senza chiedere i volumi di un'installazione precedente. **Elimina i dati di quell'installazione.** |

`OTW_DIR` e `OTW_REF` impostano gli stessi valori di `--dir` e `--ref` (un flag prevale sulla sua variabile), e `OTW_REPO` il repository GitHub (predefinito `G-OTW/OpenTraderWorld`).

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

Accetta anche queste opzioni:

| Opzione | Note |
|---|---|
| `--build` | Compila le immagini dai sorgenti invece di scaricarle (vedi sotto). |
| `--yes` | Prende ogni valore predefinito quando non c'è un terminale da cui chiedere. |
| `--wipe-volumes` | Rimuove senza chiedere i volumi di un'installazione precedente. **Elimina i dati di quell'installazione.** |
| `--non-interactive` | Come `OTW_NONINTERACTIVE=1`: legge ogni risposta dall'ambiente, mai da un terminale. Una variabile non impostata prende il valore predefinito. |

Le variabili non interattive, una per domanda: `NET_CHOICE` (da `1` a `4`), `OTW_HTTP_PORT`, `ADMIN_USER`, `OTW_ADMIN_PASSWORD` (generata se non impostata), `OTW_LOG`, `DUCK_DOMAIN`, `DUCK_TOKEN` e `LAN_IP` (modalità 3), `OTW_DOMAIN` (modalità 4), e `OVERWRITE`, `CLEAN_VOLS`, `WIPE_VOL`, `START_NOW` (`y` o `N`) per le conferme.

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
- `OTW_SECRET_KEY`: un'altra lunga stringa casuale, la chiave che cifra le credenziali salvate. Mantienila stabile: cambiarla in seguito le rende illeggibili.

Compose ha bisogno anche dei due file di rete che `setup.sh` avrebbe scritto. Per la modalità localhost sulla porta 5454:

```bash
cat > network.env <<'EOF'
OTW_BIND=127.0.0.1
OTW_HTTPS_BIND=127.0.0.1
OTW_HTTP_PORT=5454
OTW_HTTPS_PORT=8443
OTW_DOMAIN=:80
EOF
printf 'OTW_ACME_GLOBAL=\n' > dns.env
```

Entrambi vengono poi riscritti da **Impostazioni → Rete**, vedi [Rete e accesso remoto](/it/config/network).

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

Altrimenti apri l'app nel browser (`http://localhost:5454` per impostazione predefinita). Alla prima visita OpenTraderWorld rileva che non c'è ancora un admin e mostra la **procedura guidata di configurazione**: scegli nome utente e password, invia, e arrivi sulla dashboard. La password richiede almeno 12 caratteri, e viene rifiutata se è un carattere ripetuto, una sequenza progressiva, se contiene il nome utente o se compare in un elenco di password violate note. Le password sono salvate con hash argon2.

Nelle modalità HTTPS (LAN + HTTPS, pubblica) la procedura guidata chiede anche un **token di configurazione**, così chi trova soltanto l'indirizzo non può reclamare l'account per primo. Il core lo stampa nel suo log all'avvio: `docker compose logs core`.

::: details Creare l'admin dalla CLI (headless, senza browser)
Chiama l'endpoint di primo avvio dall'interno dello stack: funziona qualunque sia l'interfaccia di bind, e rifiuta (HTTP 409) se esiste già un admin. Nelle modalità HTTPS aggiungi come ulteriore header il token di configurazione preso dal log del core, `--header=X-OTW-Setup-Token:<token>`, altrimenti risponde 403 `setup_token_required`:

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
- Health check: `http://localhost:5454/api/health` → `{"status":"ok"}` (con accesso effettuato restituisce anche la versione, l'uptime e lo stato di ogni servizio)

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## Operazioni quotidiane

Eseguile da `deploy/`. Ogni comando prende entrambi i file env, e un'installazione da immagini prende anche l'override delle immagini, quindi parti dalla riga che corrisponde alla tua installazione:

```bash
# image install (the default)
docker compose -f docker-compose.yml -f docker-compose.images.yml --env-file .env --env-file network.env
# source build
docker compose --env-file .env --env-file network.env
```

| Azione | Aggiungi |
|---|---|
| Avviare | `up -d` |
| Fermare | `down` |
| Vedere i log | `logs -f` |
| Ricompilare dopo modifiche al codice (build da sorgenti) | `up --build -d` |
| Fermare **ed eliminare tutti i dati** | `down -v` |

Per passare a una versione più recente segui [Aggiornamento](/it/guide/updating): i tag delle immagini sono fissati in `docker-compose.images.yml`, quindi un semplice `pull` scarica la versione che già usi.

I tuoi dati vivono nei volumi con nome di Docker e **persistono** tra `up`/`down`. Vengono eliminati solo con `down -v`.

## Prossimi passi

- [Primi passi](/it/guide/first-steps): accedi, imposta i valori predefiniti, installa i moduli.
- [Rete e accesso remoto](/it/config/network): raggiungi l'app da altri dispositivi, HTTPS in LAN, esposizione pubblica.
- Qualcosa non va? Vedi [Risoluzione dei problemi](/it/guide/troubleshooting).
