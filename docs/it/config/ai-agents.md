# Agent IA (MCP)

OpenTraderWorld include un **server MCP** integrato, così gli agent IA (qualsiasi client compatibile con [MCP](https://modelcontextprotocol.io)) possono leggere e aggiornare i tuoi moduli tramite un gateway controllato. Un agent può registrare operazioni nel journal per te, riassumere i tuoi feed di notizie, aggiungere todo, interrogare i risultati dei tuoi backtest, e così via.

**È disattivato per impostazione predefinita.** Nulla è in ascolto per gli agent finché non lo abiliti.

::: tip Cerchi l'assistente chat dentro l'app?
Questa pagina riguarda gli agent **esterni** che si collegano *a* OpenTraderWorld. Se vuoi l'assistente chat integrato che vive dentro l'app (porta il tuo provider), vedi il [modulo Agent](/it/modules/agent): può *usare* questo stesso gateway per raggiungere i tuoi dati.
:::

## Modello di sicurezza {#security-model}

Più livelli, tutti da superare:

1. **Interruttore globale**: l'endpoint MCP è disabilitato finché non lo abiliti in **Impostazioni → MCP**. Puoi preparare i token mentre è spento; ogni richiesta degli agent viene rifiutata finché non è abilitato.
2. **Bearer token**: uno per agent o caso d'uso. I token sono salvati con **hash** e mostrati **una sola volta** alla creazione; i tentativi falliti vengono rallentati. Revoca un token in qualsiasi momento.
3. **Permessi per modulo per token**: ogni token concede *nessun accesso*, *lettura*, *lettura + scrittura* o *completo (lettura + scrittura + eliminazione)* **per modulo**. Gli agent scoprono solo i moduli che hai concesso.
4. **Allowlist rigida**: le operazioni su account, rete, segreti, archiviazione di file e cancellazione dati non sono **mai esposte** agli agent, a prescindere dai permessi.

::: tip L'allowlist è deliberata, non automatica
Un endpoint è raggiungibile dagli agent solo perché qualcuno lo ha aggiunto a mano al catalogo. Un nuovo modulo, o una nuova route in uno esistente, è **invisibile a ogni agent** finché quella voce non esiste, così il gateway non può mai allargarsi per caso man mano che l'app cresce. La gestione di persona e skill è tenuta fuori di proposito: nessun agent, e nessun contenuto letto da un agent, può modificare una persona o ampliare il suo scaffale di skill.
:::

::: tip Le versioni funzionano come i commit
Con il versionamento attivo in **Impostazioni → Versioni**, un agent con permesso su **Editor** o **Backtest** può salvare una versione di un documento o di una strategia dopo averlo aggiornato, con una nota che dice cosa è cambiato, ed elencare, leggere, ripristinare o eliminare versioni. Può attivare il versionamento per singolo file o strategia, ma non gli interruttori globali nelle Impostazioni.
:::

::: warning L'Automator concede la scrittura, non l'armamento
Concedere **Automator** permette a un agent di leggere i tuoi workflow, crearne uno, scriverne il grafo e testarlo. **Non** gli permette di metterne uno in servizio: un grafo salvato da un agent resta una bozza che adotti dall'editor, non può collegare il token di accesso di un workflow, e non può eseguire un workflow né toccare una pianificazione. Un workflow gira con il proprio token invece che con quello del chiamante, quindi scrivere un grafo e armarlo sono due permessi distinti. Vedi [la pagina del modulo](/it/modules/automator#letting-an-agent-build-a-workflow).
:::

## Abilita e crea un token

1. Vai in **Impostazioni → MCP** e attivalo.
2. **Nuovo token**: dagli il nome del client (ad es. `My Agent`), imposta i permessi per modulo (oppure usa *Tutti in lettura* / *Tutti lettura+scrittura* / *Tutti completi* come punto di partenza).
3. **Copia subito il token**: viene mostrato una sola volta.

**Consenti l'accesso esterno** è una spunta separata nella stessa finestra. Non concede nulla in più: permette solo a quel token di sostenere un binding di chat in [Controllo esterno](/it/config/external-control), dove un messaggio da Telegram, Slack o Discord gira con questi stessi livelli per modulo.

La finestra di creazione mostra anche uno **snippet di configurazione pronto da incollare**, con una scheda per famiglia di client: endpoint grezzo + header, un blocco JSON `mcpServers` (Cursor, Cline, Windsurf, VS Code…) e una riga di comando `claude mcp add`. Gli stessi snippet restano disponibili sotto la tabella dei token con `<TOKEN>` come segnaposto, per configurare una seconda macchina in seguito.

## Collega un client

L'endpoint parla **MCP su Streamable HTTP** a:

```
POST http://<your-host>/api/mcp
Authorization: Bearer <TOKEN>
```

Funziona qualsiasi client conforme. Esempio per una configurazione MCP:

```json
{
  "mcpServers": {
    "opentraderworld": {
      "type": "http",
      "url": "http://localhost:5454/api/mcp",
      "headers": { "Authorization": "Bearer <TOKEN>" }
    }
  }
}
```

Sostituisci l'URL con il tuo dominio se usi una modalità LAN/HTTPS.

::: tip Installazioni solo localhost
Se l'app è raggiungibile solo su `localhost` (la modalità di rete predefinita), gli agent devono girare **sulla stessa macchina**.
:::

## Collegarsi con OAuth (claude.ai, ChatGPT)

Alcuni client non possono tenere un token fisso: i connector di claude.ai e ChatGPT accedono solo con OAuth. Per loro, attiva il **sign-in OAuth** in fondo a **Impostazioni → MCP** (chiede la tua password) e dai al client solo l'URL del server, `https://<your-domain>/api/mcp`, senza token.

1. Il client si registra da solo e apre una pagina di consenso sulla tua istanza (accedi prima, se serve).
2. La pagina mostra il nome del client e **dove viene inviata la tua risposta**. Scegli moduli e livelli, poi **Consenti**: la tua password viene richiesta a ogni approvazione.
3. La connessione compare nella tabella dei token con un badge **OAuth**. Modifica i suoi permessi o revocala lì come qualsiasi token; revocare disconnette il client.

I token di accesso durano un'ora e vengono rinnovati in background; una connessione inutilizzata per 30 giorni deve accedere di nuovo. Se un token di rinnovo viene mai riutilizzato da qualcun altro, la connessione viene revocata e vieni avvisato.

::: warning I client remoti richiedono HTTPS pubblico
claude.ai e ChatGPT si collegano dai propri server, quindi l'istanza deve essere raggiungibile in HTTPS pubblico ([modalità Web](/it/config/network)). Approva solo una pagina di consenso che hai aperto tu, proprio adesso: un link inviato da qualcun altro può spacciarsi per qualsiasi client.
:::

::: tip Aggiornato dalla 0.0.15 o precedente con `otw update`?
OAuth richiede una nuova route in `deploy/Caddyfile`. Vedi [Aggiornamento](/it/guide/updating#oauth-caddyfile).
:::

## Come gli agent vedono l'app

Gli agent ottengono quattro strumenti del gateway:

- **`otw_catalog`**: elenca i moduli e le operazioni che il token può chiamare. Compaiono solo i moduli concessi. L'elenco di un modulo mostra metodo, percorso, parametri di query e campi di primo livello del body di ogni operazione; `endpoint` (`POST /api/backtest/run`) restituisce lo schema completo del body di quella operazione.
- **`otw_read`**: operazioni di lettura (richiedono almeno *lettura* sul modulo).
- **`otw_compute`**: operazioni contrassegnate *(compute)* nel catalogo, che rispondono a una domanda e non salvano nulla: un backtest, uno sweep di parametri, metriche di rischio. Richiedono *lettura + scrittura* come qualsiasi POST, ma il tuo client non ti chiederà di approvare un calcolo.
- **`otw_write`**: operazioni di creazione e aggiornamento (richiedono *lettura + scrittura*); le operazioni di **eliminazione** richiedono *completo* sul modulo.

Le risposte che portano testo dall'esterno (articoli dei feed, posta in arrivo) arrivano all'agent dentro un blocco etichettato che gli dice di trattare il contenuto come dato e di ignorare qualsiasi istruzione nascosta al suo interno.

La tabella dei token nelle Impostazioni mostra l'ultimo utilizzo di ogni token, così puoi individuare e revocare quelli inattivi. A un token si può anche dare una data di scadenza alla creazione o alla modifica: dopo quella data smette di funzionare ovunque, anche per l'agent integrato nell'app.
