# Risoluzione dei problemi

| Sintomo | Causa probabile / soluzione |
|---|---|
| `port is already allocated` | La porta (80/443/5454) è usata da qualcos'altro. Riesegui `./setup.sh` e scegli una porta diversa. |
| Chrome/Edge non apre l'app ma Safari sì | Il browser forza l'indirizzo a `https://`, che le modalità HTTP semplice non servono. Digita `http://` esplicitamente, oppure passa alla [modalità LAN + HTTPS](/it/config/network#lan-https). |
| Funziona su `localhost` ma non tramite l'IP della macchina (macOS) | Il firewall di macOS blocca le connessioni in ingresso di Docker. Impostazioni di Sistema → Rete → Firewall → Opzioni… → imposta **Docker** su *Consenti connessioni in entrata*. |
| LAN + HTTPS: certificato non emesso | Controlla `docker compose logs caddy`. Il token DuckDNS/Cloudflare deve essere valido e il dominio scritto esattamente. |
| LAN + HTTPS: il dominio non si risolve su alcuni dispositivi | Il tuo resolver blocca le risposte con IP privati (protezione DNS rebind). Vedi [le soluzioni](/it/config/network#dns-rebind). |
| La procedura di configurazione non compare mai / `core: offline` nella barra in alto | Il core non raggiunge Postgres. Controlla `docker compose logs core` e `logs postgres`; verifica che `DATABASE_URL` corrisponda a `POSTGRES_PASSWORD` in `deploy/.env`. |
| Errore `POSTGRES_PASSWORD` all'avvio | `deploy/.env` manca o è vuoto. Esegui `./setup.sh`, oppure copia `.env.example` in `.env` e compilalo. |
| Modalità pubblica: certificato HTTPS non emesso | Il DNS del dominio deve risolvere verso questo server, e le porte 80/443 devono essere raggiungibili da internet. |
| Le modifiche al codice non si vedono | Ricompila: `docker compose up --build -d`. |
| Bloccato fuori, password dimenticata | Reimpostala dalla shell dell'host, vedi [Password dimenticata](#forgot-password). |
| Non raggiungo l'app dopo aver scelto la modalità di rete sbagliata | Modifica `deploy/network.env` a mano e riavvia, vedi [cambiare la modalità dalla CLI](/it/config/network#change-mode-cli). |

## Password dimenticata {#forgot-password}

Non c'è email di reset né un modulo di reset non autenticato: OTW gira sul tuo server, quindi qualsiasi endpoint che potesse cambiare una password senza essere connessi sarebbe una porta d'ingresso. Il reset vive invece nella **shell dell'host**, e il link *Password dimenticata?* della pagina di login riporta gli stessi passaggi.

Apri una shell sulla macchina che esegue OTW e stampa una password monouso:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core reset-password USERNAME
```

Accedi con essa; l'app te ne chiede subito una nuova.

| Caso | Cosa eseguire |
|---|---|
| Hai dimenticato anche il nome utente | `docker exec opentraderworld-core-1 /app/otw-core list-users` |
| Scegli tu la password | `printf '%s' 'my-new-password' \| docker exec -i opentraderworld-core-1 /app/otw-core reset-password USERNAME --stdin` |
| Container con un nome diverso | `docker ps`, poi usa quello del core al posto di `opentraderworld-core-1`. |
| Non usi Docker | Esegui il binario `otw-core` con gli stessi argomenti e `DATABASE_URL` impostato. |

Non passare mai una password come argomento da riga di comando: la riga di comando di un processo è leggibile sull'host, ed è per questo che esiste `--stdin`.

Note: un reset **disconnette ogni dispositivo**, e non si perde nulla. Il vault e le chiavi dei provider salvate sono sigillati con `OTW_SECRET_KEY`, non con la tua password.

## Autenticatore perso {#lost-authenticator}

Stesso principio della password: recupera dalla shell dell'host.

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

Il secondo fattore viene rimosso e ogni sessione viene disconnessa. Accedi con la password,
poi configuralo di nuovo sul nuovo dispositivo da **Impostazioni → Sicurezza**. Vedi
[Autenticazione a due fattori](/it/config/security#totp).

## Bloccato fuori dall'social login {#social-locked}

L'account del provider collegato è bloccato, eliminato o irraggiungibile: nella pagina di login, **Usa un
codice di recupero**. L'sign-in con password si riattiva.

Perso anche i codici di recupero, sull'host:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Rimuove il collegamento, riattiva l'sign-in con password e disconnette ogni sessione. Persa anche la
password: [Password dimenticata](#forgot-password). Dettagli: [Social login](/it/config/social-login#rollback).

## Una richiesta lunga viene interrotta {#request-timeout}

Un backtest, uno sweep o un grosso import che risponde con *this request took longer than the
Ns limit* ha raggiunto il timeout delle richieste, non è un bug. Alzalo in **Impostazioni → Sicurezza**; si applica
subito, senza riavvio. Vedi [Timeout delle richieste](/it/config/security#timeout).

## Leggere i log

```bash
cd deploy
docker compose ps              # are all containers up?
docker compose logs -f core    # API server
docker compose logs -f caddy   # proxy / certificates
docker compose logs -f postgres
```

L'app tiene anche una propria vista dei log in **Impostazioni → Log** (ricercabile, con un livello di cattura configurabile).

## Ripartire da zero

::: danger Questo elimina tutti i dati
```bash
cd deploy
docker compose down -v
./setup.sh
```
:::

## Ancora bloccato?

Apri una issue su [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues) con il sintomo e le righe di log pertinenti.
