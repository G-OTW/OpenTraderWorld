# Notizie e ricerca

## News {#news}

Un aggregatore di notizie self-hosted. Crea **dashboard** (ad es. *Crypto*, *Macro*), aggiungi **fonti** a ciascuna, e lascia che lo scheduler le interroghi in background.

### Fonti

- **RSS / Atom**: incolla l'URL di un feed, fatto.
- **API (JSON)** per qualsiasi cosa senza RSS: imposta endpoint, metodo, header e parametri di query, poi mappa i percorsi JSON ai campi dell'elemento (array di elementi, titolo, URL, data, riassunto, id univoco per la deduplicazione). Le chiavi API vanno nei **segreti** per feed, salvati cifrati e referenziati come <code v-pre>{{secret:NAME}}</code> in header o parametri, e non vengono più mostrati.

L'URL, un header o un parametro di un feed può anche portare un segnaposto <code v-pre>{{vault.item}}</code> che punta al [Vault](/it/config/settings#vault) condiviso. Lo scheduler lo risolve al momento del polling, così una chiave è riutilizzata tra feed e moduli senza mai essere salvata nella configurazione del feed.

Ogni fonte ha il proprio **intervallo di polling**; le fonti duplicate vengono rilevate così lo stesso feed non viene scaricato due volte tra dashboard. Avvia/ferma il polling per dashboard, o aggiorna una fonte su richiesta.

### Lettura

Filtra gli elementi per ricerca, fonte, tipo e intervallo di date; vista compatta o completa; aggiornamento automatico opzionale ogni 60 secondi con un banner "{n} aggiornamenti, clicca per caricare". Un widget news può stare anche nella home della tua dashboard.

## Mailbox {#mailbox}

Le tue newsletter, la posta di notizie di mercato e la posta del broker, lette dalla **tua casella**: nulla transita da terzi.

### Collegare una casella

Scegli il tuo provider (Fastmail, Gmail, iCloud, Zoho, mailbox.org, Posteo, Migadu, Proton Bridge o qualsiasi altro server IMAP) e le impostazioni del server arrivano precompilate; tu fornisci una **password per app**, che viene salvata nel [Vault](/it/config/settings#vault) condiviso e in nessun altro posto.

**Outlook.com / Microsoft 365** non accettano più una password per IMAP, quindi accedono con OAuth: si apre una scheda su Microsoft, approvi l'accesso, e torna direttamente a questa app (authorization code + PKCE, nessun segreto viene salvato da nessuna parte). Serve una registrazione app gratuita una tantum, tua: Entra ID → App registrations → nuova registrazione, poi Authentication → *Mobile and desktop applications* con l'URI di reindirizzamento che ti mostra il modulo (`http://localhost:5454/mailbox/oauth` in un'installazione locale predefinita), public client flows consentiti, e API permissions → delegated `IMAP.AccessAsUser.All`. Incolla l'Application (client) ID nel modulo. Il sign-in risultante è cifrato nel vault e rinnovato automaticamente a ogni fetch.

Microsoft accetta solo un URI di reindirizzamento `https://…`, oppure `http://` su localhost, e il suo portale rifiuta un URI `http` digitato come `127.0.0.1`, quindi apri l'app su `http://localhost:5454` (la porta viene ignorata nel confronto con un redirect localhost) oppure mettila dietro HTTPS in [Impostazioni → Rete](/it/config/settings#network). Se il tuo è un indirizzo LAN in HTTP semplice, il sign-in ripiega su un codice che digiti su `microsoft.com/devicelogin`; funziona ancora per gli account personali Outlook.com, ma i tenant Microsoft 365 ora bloccano per impostazione predefinita il sign-in con device code.

Quel rinnovo è l'unica cosa da sapere sulla manutenzione: Microsoft scarta un sign-in dopo **90 giorni senza utilizzo**, quindi una casella che hai messo in pausa per mesi chiederà di essere ricollegata. L'app avvisa dopo 60 giorni di inattività e, se il sign-in viene revocato (cambio password, reset MFA, policy dell'admin), la casella mostra **Sign-in needed** con un pulsante Ricollega invece di fallire in silenzio.

L'accesso è rigorosamente **in sola lettura**: la cartella viene aperta in sola lettura, e nulla viene mai contrassegnato, spostato o eliminato sul tuo server. Collega più caselle se ne hai più di una.

> Valuta un **indirizzo dedicato** per le newsletter. La tua posta personale resta così fuori dall'app, la password per app è revocabile con un clic, e il giorno in cui un mittente fa trapelare la sua lista sai esattamente quale è stato.

### Cosa viene conservato

La posta di mailing list (tutto ciò che porta `List-Unsubscribe`, `List-Id` o `Precedence: bulk`) viene conservata automaticamente. Tutto il resto viene solo *registrato come mittente in attesa della tua decisione*, e nessun contenuto viene salvato finché non lo archivi. È così che entrano gli estratti di un broker: un clic sul nuovo mittente, archiviato come **Broker**.

I mittenti sono archiviati in quattro categorie (**News**, **Newsletter**, **Broker**, **Altro**), cambiabili in qualsiasi momento, e la schermata di lettura ha un interruttore con un clic per categoria più un filtro per casella quando ne hai più di una.

### Lettura

I messaggi sono sanificati all'arrivo (script, stili, moduli e frame rimossi) e mostrati in un frame in sandbox. **Le immagini remote restano bloccate** finché non le richiedi, così il pixel di tracciamento in una newsletter non scatta mai e il mittente non può sapere che l'hai aperta. Gli allegati (estratti del broker, PDF) sono scaricabili dal messaggio.

Per messaggio: stella, segna come non letto, archivia, **Ricordamelo** (stasera / domani / questo weekend, direttamente in [RemindMe](/it/modules/productivity#remindme)) e **Annulla iscrizione**, inviata per te quando il mittente supporta il one-click, aperta in una scheda altrimenti.

### Lo store

La scheda **Store** è il tuo elenco di newsletter: una scheda per pubblicazione con nome, link, breve descrizione e argomento (mindset, finanza, trading, geopolitica, economia, altro), raggruppate per dominio e apribili con un clic. Sta in piedi da sola, utile anche senza alcuna casella collegata.

## Economic Calendar {#economics}

Prossimi eventi macro (decisioni delle banche centrali, dati CPI, dati sull'occupazione) in una vista calendario, così sai cosa ti aspetta nella tua sessione. Un clic aggiunge un promemoria per un evento.

## FinanceDatabase {#findb}

Un catalogo ricercabile di **oltre 300.000 strumenti**: azioni, ETF, fondi, indici, valute e criptovalute.

Al primo utilizzo **installi il catalogo** (un download una tantum di ~15 MB, importato in background). Dopo vive in locale e **le ricerche non toccano mai la rete**. Cerca per simbolo o nome, filtra per tipo di asset e attributi, e metti le stelle agli strumenti nei **preferiti**, organizzati in cartelle con note (ad es. una cartella *Watchlist*).

Il catalogo ha un proprio ciclo di rilascio, separato dall'app: l'intestazione mostra quale **snapshot** è installato e un pulsante **Cerca aggiornamenti** chiede all'editore se ne esiste uno più recente. L'aggiornamento reimporta il catalogo sul posto; i tuoi preferiti sopravvivono e si ricollegano alle nuove righe.

Il catalogo è costruito dal progetto open source [FinanceDatabase](https://github.com/JerBouma/FinanceDatabase) di Jeroen Bouma, un dataset di strumenti finanziari mantenuto dalla community.

## Resources {#resources}

Una libreria di segnalibri per libri di trading, articoli, video e strumenti: nome, link opzionale, descrizione, organizzati in categorie. Semplice di proposito.

Tre visualizzazioni: **schede**, **elenco** e una **galleria** con una miniatura per segnalibro. Una miniatura si carica, si incolla come URL, o si recupera con un clic dall'anteprima social del link stesso; un segnalibro senza miniatura ottiene una tessera con le iniziali invece di un buco nella griglia.

## Community Docs {#community-docs}

Guide scritte dalla community, sincronizzate dalla [libreria su opentraderworld.com](https://opentraderworld.com/docs) e **leggibili offline** dentro l'app. Sfoglia per categoria, cerca e metti le stelle ai preferiti.

I documenti appaiono come **schede o come elenco**, a tua scelta, e una scheda di categoria mostra in anteprima i documenti che contiene così sai cosa c'è dentro prima di aprirla.

Puoi contribuire: scrivi un documento nell'[Editor](/it/modules/productivity#editor) e usa **Invia per la pubblicazione**. Va in una coda di revisione e compare nella libreria di tutti una volta approvato.
