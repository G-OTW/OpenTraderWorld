# Riferimento delle impostazioni

Tutto ciò che sta sotto la voce **Impostazioni** del selettore dei moduli, sezione per sezione.

## Account {#account}

Cambia nome utente o password. La password attuale è richiesta per salvare le modifiche, e cambiare la password **ti disconnette da tutte le sessioni**.

Una nuova password deve avere almeno 12 caratteri e viene rifiutata se compare negli elenchi pubblici di violazioni. Vedi [Regole per la password](/it/config/security#password).

## Sicurezza {#security}

Autenticazione a due fattori, i browser attualmente connessi al tuo account e per quanto tempo può durare una singola richiesta. Trattato in dettaglio in [Sicurezza dell'account](/it/config/security).

## Predefiniti {#defaults}

- **Lingua**: si applica subito a tutta l'app (en, fr, de, es, it, pt, zh).
- **Valuta predefinita** e **fuso orario**: i valori iniziali che i moduli usano per i nuovi elementi e per la visualizzazione.

## Aspetto {#appearance}

Il **colore di accento** dell'app, quello usato da pulsanti primari, stati attivi, link ed evidenziazioni dei grafici. Scegli una tinta predefinita o un colore qualsiasi dal selettore; si applica subito in tutta l'app ed è salvato per ogni sessione. *Ripristina* lo riporta al predefinito del tema.

## Rete {#network}

Chi può raggiungere l'app: localhost, LAN, LAN + HTTPS o pubblico. Trattato in dettaglio in [Rete e accesso remoto](/it/config/network).

## Vault {#vault}

Un unico posto per le chiavi API e i segreti che l'app usa per tuo conto, invece di incollare la stessa chiave in ogni modulo che la richiede.

Un **vault** rappresenta un servizio esterno (ad es. *Binance*) e contiene **chiavi** con nome: `apikey`, `secretkey` e così via. Crea quante vault e chiavi ti servono; i moduli poi **collegano una chiave per riferimento** tramite un selettore condiviso, ovunque venga richiesto un segreto (connector dei provider, credenziali dei feed…).

- **Valori in sola scrittura.** Un segreto viene sigillato al salvataggio e non può più essere visualizzato, solo sostituito o eliminato. I *nomi* delle chiavi restano visibili. Tutto è cifrato a riposo con la chiave principale dell'app.
- **Scollega prima di eliminare.** Eliminare un vault o una chiave ancora collegati a un modulo è bloccato; rimuovi prima il riferimento lì. Ogni vault mostra quante connessioni lo usano.
- Il **tracciamento delle richieste** è opzionale e **per vault, non per chiave**: tutte le chiavi di un vault contano sullo stesso contatore. Il limite è solo informativo (osserva e mostra); nulla viene mai limitato. Alimenta la stessa vista di [Frequenza API](#api-rate).

I segreti dei feed di notizie accettano anche segnaposto inline <code v-pre>{{vault.item}}</code>, risolti dallo scheduler al momento del polling. Vedi [Notizie](/it/modules/news-research#news).

## Moduli {#modules}

Installa e scollega i moduli. Tutto è incluso nell'app: installare rende solo un modulo disponibile nel selettore e nella dashboard; non viene scaricato nulla. Scollegare lo nasconde e lo rende inaccessibile; spunta *elimina anche i dati* per cancellare anche i suoi dati salvati (permanente).

## Gestisci dati {#manage-data}

Uso dello spazio per modulo (tabelle, righe, dimensione) con il totale del database, e un'azione **Cancella** per eliminare definitivamente i dati di un modulo (digita il suo nome per confermare). La cancellazione non si può annullare.

## Versioni {#versioning}

Due interruttori: **File dell'editor** e **Strategie**, ciascuno con il numero di versioni salvate e la loro dimensione. Attivarne uno avvisa che ogni versione è una copia completa e che il database cresce a ogni versione (immagini e video non vengono mai duplicati). Disattivarne uno chiede se mantenere le versioni (nascoste finché non lo riattivi) o eliminarle tutte. Una volta attiva un'area, il versionamento si attiva per singolo file o per singola strategia dal menu della sua cronologia: vedi [Editor](/it/modules/productivity#editor) e [Backtest](/it/modules/market-data#strategies-and-custom-indicators).

## Backup e ripristino {#backup-restore}

Due schede, ciascuna con un lato **Backup** e uno **Ripristina**:

- **Completo**: comandi `pg_dump` e `psql` pronti da copiare per il tuo deployment, comprese le varianti cifrate, più lo stato del backup automatico sulle istanze che ne eseguono uno.
- **Parziale**: scegli i moduli che vuoi, scaricali come un unico zip e ricarica quello zip qui o su un'altra istanza. Entrambi i lati sono contati prima: cosa stai prelevando, per modulo e per tabella, e cosa contiene un file che carichi rispetto a ciò che c'è già.

Vedi [Backup e ripristino](/it/guide/backup-restore).

## Aggiorna app {#update-app}

Mostra la versione attuale, controlla su GitHub se ce n'è una più recente ed elenca i comandi di aggiornamento da eseguire sull'host. Vedi [Aggiornamento](/it/guide/updating).

## Log {#logs}

L'archivio dei log dell'app, ricercabile per messaggio/target. Il **livello di cattura** imposta la severità minima scritta nell'archivio (ha effetto subito). Livelli più bassi catturano più dettaglio e usano più spazio. Qui puoi cancellare i log salvati.

## Frequenza API {#api-rate}

Una dashboard delle chiamate in uscita verso provider di dati esterni (dati di mercato, FX, quotazioni, feed), contate per giorno UTC: numero di richieste per provider, errori, risposte di rate limit, limiti pubblicati dove noti, e un elenco dei rate limit raggiunti di recente. Esiste per vedere quanto sei vicino ai limiti dei piani gratuiti di un provider.

**Questa pagina non limita nulla**: osserva soltanto. L'unico punto in cui un limite viene davvero applicato è il [limite di richieste di un connector](/it/config/connectors#request-limits) sui fetch su richiesta del grafico; ovunque altrove un limite informa e avvisa, e resta il provider a dire no.

## Data connector {#data-connectors}

L'elenco condiviso degli account dei provider di dati di mercato usati da Historical Data, Visualization, Watchlists e dal Trading Journal: credenziali, limiti di richieste e quali moduli possono usare ciascuno. Trattato in [Data connector](/it/config/connectors). La stessa schermata è disponibile anche da sola su **/connectors**, e dal pulsante del connector dentro ogni modulo dati.

## Broker {#brokers}

L'elenco condiviso degli **account broker in sola lettura** usati da Trading Journal, Portfolios, Visualization e Tax Calculator: credenziali, impostazioni per broker e quali moduli possono usare ciascuno. Trattato in [Conti broker](/it/config/brokers). Nulla qui può inserire, modificare o annullare un ordine.

## Notifiche {#notifications}

L'elenco condiviso dei **canali di notifica**: dove l'app è autorizzata a inviare, creati una volta e riutilizzati da ogni modulo che notifica.

Un canale è una destinazione **di tua proprietà**. Ciascuno contiene un segreto, digitato qui o collegato dalla [Vault](#vault), sigillato al salvataggio e mai più mostrato.

| Canale | Cosa porti | Segreto | Altri campi |
|---|---|---|---|
| **Email** | il tuo server SMTP | password | host, porta (587 STARTTLS, 465 TLS), mittente, destinatario, nome utente |
| **Telegram** | un bot BotFather | token del bot | chat id |
| **Slack** | un Incoming Webhook | l'URL del webhook | nessuno |
| **Discord** | un Webhook di canale | l'URL del webhook | nessuno |

Tutti e quattro sono gratuiti per l'host: porti tu l'account, l'app non porta nulla a cui registrarsi. Alcuni campi **non segreti** accettano anche un elemento del vault, il **chat id** di Telegram per esempio, così un canale può essere configurato senza che quell'id stia in chiaro nella configurazione.

I moduli a cui si può concedere un canale:

| Modulo | Cosa invia |
|---|---|
| **RemindMe** | un promemoria scattato |
| **Watchlists** | un avviso di prezzo |
| **Mailbox** | un account email che richiede attenzione |
| **Webhooks** | un payload in entrata reindirizzato a un modulo |
| **Historical Data** | un parcheggio lungo e la fine di un lotto di download |
| **Visualization** | un avviso del grafico scattato |
| **Journal** | l'arricchimento con dati di mercato di una nuova operazione |
| **Backtest** | un fill di paper trading, o il suo riepilogo raggruppato |
| **Portfolio Tracker** | concedibile in anticipo rispetto a ciò che invierà; oggi non invia nulla |
| **Automator** | qualunque cosa invii un blocco `notify` |

- **Permessi, per modulo.** Ogni canale indica i moduli autorizzati a inviarvi, oppure *tutti*. Il controllo viene eseguito lato server: un modulo a cui non è mai stato concesso un canale non può raggiungerlo, e il segreto di quel canale non viene nemmeno decifrato per lui.
- **Un interruttore per canale.** Disattivare un canale lo silenzia ovunque senza eliminarlo né eliminare le sue credenziali.
- **Invio di prova** prima di fidarti di uno.

La stessa schermata si apre dall'interno di ogni modulo che notifica, così un canale si può aggiungere al volo senza lasciare la pagina in cui sei. È deliberatamente assente dal catalogo [MCP](#mcp): nessun agent può creare un canale o ampliare un permesso.

## MCP {#mcp}

Permetti agli agent IA di usare l'app tramite un gateway controllato. Trattato in [Agent IA (MCP)](/it/config/ai-agents).

## Controllo esterno {#external-control}

Pilota l'app da un canale chat (Telegram, Slack, Discord). Trattato in [Controllo esterno (chat)](/it/config/external-control).

## Voce {#voice}

Comandi push-to-talk e dettatura: il motore vocale, le due scorciatoie e i tuoi comandi vocali. Trattato in [Controllo vocale](/it/config/voice). Il microfono funziona solo su HTTPS o su localhost.

## Crediti {#credits}

Le fonti di dati e i progetti upstream che ogni modulo può usare, compresi i provider che non hai configurato.

## Informazioni {#about}

Versione, link del progetto e pulsanti di condivisione.
