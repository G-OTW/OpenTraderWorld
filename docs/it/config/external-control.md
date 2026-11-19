# Controllo esterno (chat)

Pilota OpenTraderWorld da **Telegram, Slack o Discord**. Un messaggio che invii al tuo bot diventa un'esecuzione di una delle tue [persone dell'agent](/it/modules/agent), risposta in chat, con esattamente l'accesso che permette il token scelto.

**È disattivato per impostazione predefinita**, e non aggiunge alcun sistema di permessi proprio: il tetto è un [token MCP](/it/config/ai-agents), lo stesso che usa l'assistente nell'app.

## Nulla di nuovo ascolta sulla tua macchina

Tutti e tre i trasporti **chiamano verso l'esterno**: long polling di Telegram, Socket Mode di Slack, il gateway di Discord. Non c'è un URL pubblico da pubblicare, nessuna porta da aprire e nessuna route in entrata da attaccare, quindi funziona senza modifiche sull'installazione predefinita solo-localhost e dietro NAT.

Il canale che già usi per le notifiche può solo **inviare** (l'URL di un webhook Slack o Discord è in sola scrittura). Ricevere richiede un vero bot, quindi un binding porta la propria credenziale:

| Piattaforma | Credenziale da incollare | Sulla piattaforma |
|---|---|---|
| **Telegram** | il token del bot di BotFather | nient'altro |
| **Slack** | **entrambi** i token, `xapp-…` e `xoxb-…`, separati da uno spazio o un a capo | Socket Mode attivo, evento `message.im`, scope `chat:write` |
| **Discord** | il token del bot | l'intent **Direct Messages** (l'intent privilegiato del contenuto dei messaggi non serve per i DM) |

## Configurane uno

1. **Impostazioni → Notifiche**: crea il canale se non ne hai. È il percorso di risposta.
2. **Impostazioni → MCP**: crea o modifica un token, imposta i suoi livelli per modulo, e spunta **Consenti l'accesso esterno**.
3. **Impostazioni → Controllo esterno**: **Nuovo binding**, scegli il canale, la persona e quel token, incolla la credenziale del bot (o collegala dal [Vault](/it/config/settings#vault)), imposta se vuoi il provider e il modello su cui partono le nuove chat, abilita il binding.
4. Attiva l'interruttore della sezione. Il binding mostra **Connesso** entro pochi secondi.
5. **Abbina**: clicca *Abbina*, poi invia il codice di 6 cifre al tuo bot **dall'account che deve essere autorizzato a pilotare**. Funziona una volta, e dura quanto dice *Durata del codice di abbinamento* (un'ora per impostazione predefinita, da 5 minuti a un giorno).

Finché nessuno è abbinato il bot non risponde a nessuno, te compreso.

## Quale modello risponde

Ogni chat porta il proprio provider e modello, esattamente come una conversazione nell'app. Il binding imposta il **predefinito su cui parte una nuova chat**: una gamba da telefono di solito merita un modello più economico e veloce di quello che la stessa persona usa nel browser. Lascialo vuoto e una chat eredita quello della persona.

Cambia il predefinito nel modulo del binding. Cambia una chat dalla chat stessa:

- `/provider` elenca i provider configurati, `/provider 2` o `/provider openrouter` porta questa chat su uno (il che azzera il modello al predefinito di quel provider, dato che un id di modello appartiene a un solo vendor).
- `/model` elenca i modelli di quel provider, `/model 3` sceglie per posizione e `/model haiku` sceglie per testo quando corrisponde esattamente un id.

La scelta resta su quella chat e non sposta nient'altro. `/new` abbandona la conversazione, così la successiva riparte dal predefinito del binding.

## Diritti

Il token decide tutto ciò che una risposta può toccare: *nessun accesso*, *lettura*, *lettura + scrittura*, *completo* per modulo, esattamente come nella [pagina MCP](/it/config/ai-agents#security-model). Il flag `external` non allarga nulla, dice solo che quell'involucro può essere raggiunto dall'esterno.

- **Un token per binding, un binding per canale.** Revocare un token ferma quel binding e nient'altro, e la traccia dice comunque da che parte è arrivata una chiamata.
- **Usa un token dedicato**, e parti in sola lettura. Puoi allargarlo in seguito senza riabbinare.
- Un token **scaduto** o che perde il flag ferma il binding al messaggio successivo, non al riavvio successivo.

## Chi può pilotare

Una chat è un luogo, non un'identità, quindi l'autorità è vincolata all'**id del mittente** della piattaforma:

- Solo un mittente abbinato riceve risposta. Chiunque altro viene **ignorato senza risposta**, ed è voluto: un rifiuto dice a un estraneo che il bot è reale.
- I mittenti abbinati sono elencati sul binding. Cliccane uno per rimuoverlo.
- **Solo messaggi diretti.** Un gruppo permetterebbe a più persone di scrivere nel prompt di un agent che può avere un permesso di scrittura.

## Le scritture chiedono sempre

Ogni scrittura ti viene posta in chat con metodo, percorso e body esatti, e aspetta una parola. Solo `yes`, `y`, `ok`, `okay`, `approve`, `oui` o `go` la approvano; qualsiasi altra cosa, o il silenzio, la rifiuta e all'agent viene detto che è stata rifiutata.

L'impostazione **approva automaticamente le scritture** della persona non si trasferisce. È stata spuntata in una sessione autenticata nell'app; non ti segue su un telefono.

## Cosa attraversa un messaggio

In ordine, tutti:

1. l'interruttore globale in **Impostazioni → Controllo esterno**
2. il binding abilitato
3. il token che porta ancora `external`, e non scaduto
4. una chat diretta
5. il mittente presente nella allowlist
6. un rate limit di 12 messaggi al minuto per binding

Poi l'esecuzione stessa è soggetta all'allowlist del catalogo MCP: le route di account, rete, segreti, archiviazione file e cancellazione dati sono irraggiungibili qualunque cosa dica il token, e lo stesso vale per la gestione dei binding. Nessun agent può creare un binding, generare un codice di abbinamento o ampliare la propria portata.

## Note di sicurezza

- **Il token del bot è l'accesso.** Chi lo possiede può parlare con la tua istanza al livello di quel binding, con la allowlist dei mittenti ancora di mezzo. Tienilo nel [Vault](/it/config/settings#vault) e parti in sola lettura.
- **La prompt injection è il vero rischio**, non il trasporto. Il contenuto che l'agent legge (posta, feed, webhook in entrata) può portare istruzioni. Ciò che lo contiene è la stessa cosa che lo contiene nell'app: la allowlist del catalogo, i livelli del token, e la conferma delle scritture qui sopra.
- I **codici di abbinamento** sono di sei cifre, monouso e a tempo limitato, ed esistono solo tra il clic su *Abbina* e il momento in cui vengono usati. Accorcia la finestra nell'intestazione della sezione se un codice resterà sullo schermo.
- Le credenziali del bot sono sigillate a riposo e mai stampate, nemmeno nei messaggi di errore.
- Disabilitato in blocco in [modalità demo](/it/guide/demo).

## Limiti

- **Solo testo.** Niente grafici e niente file; un grafico vive ancora nell'app.
- **Nessuno streaming token per token.** Le piattaforme di chat offrono solo modifiche di messaggio, e le limitano, quindi la risposta arriva a blocchi di circa un secondo e mezzo ed è divisa al tetto della piattaforma (4096, 3000 e 2000 caratteri).
- Comandi: `/new` avvia una conversazione nuova per quella chat, `/provider` e `/model` elencano e cambiano con cosa risponde quella chat, `/whoami` mostra l'id con cui sei abbinato, `/help`.
- Un riavvio scarta una conferma di scrittura in sospeso. Nulla gira senza conferma, ti viene semplicemente chiesto di nuovo.
