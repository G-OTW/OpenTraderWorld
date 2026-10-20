# Note e organizzazione

I moduli di ogni giorno: documenti, attività, obiettivi, calendario, promemoria, più alcuni pensati per la disciplina di un trader.

## Editor {#editor}

Un editor di documenti avanzato in stile Notion. I documenti vivono in un albero di cartelle; digita `/` per il menu dei blocchi.

- **Blocchi**: titoli, elenchi, elenchi di cose da fare, citazioni, blocchi di codice, divisori, link, immagini (caricate o via URL), colore del testo, evidenziatore, dimensione del carattere, larghezza normale o piena. Salva automaticamente.
- **Database**: un tipo di documento con colonne tipizzate (testo, selezione, URL…), visualizzabile come **tabella**, **kanban** (raggruppato per una colonna di selezione) o **galleria** (con una colonna per l'immagine di copertina). Trascina per riordinare righe e colonne.
- **Invia per la pubblicazione**: invia un documento alla coda di revisione di [Community Docs](/it/modules/news-research#community-docs), formattazione preservata, con lingua, categorie e credito autore opzionale.
- **Versioni** (opt-in): attiva il versionamento in [Impostazioni → Versioni](/it/config/settings#versioning), poi per pagina o database dal suo menu della cronologia. **Salva versione** salva uno snapshot con la sua data e una nota opzionale; aprine una per vederla in sola lettura, ripristinarla (lo stato ripristinato viene salvato come una nuova versione, annotata con la data della versione ripristinata) o eliminarla. La cronologia si apre in una finestra con una ricerca su note e date; la versione che corrisponde al file attuale è segnata. Le note sono limitate a 500 caratteri. Immagini e video non vengono copiati: una versione punta agli stessi upload. Disattivare il versionamento per un file chiede se mantenere o eliminare le sue versioni. Eliminare un file elimina anche le sue versioni, dopo una conferma. Gli agent con accesso in scrittura all'Editor possono salvare, ripristinare ed eliminare versioni tramite [MCP](/it/config/ai-agents).

## ToDo {#todos}

Un elenco di attività che non dà fastidio: attività con scadenza, ora, categoria e note. Filtra per in sospeso/fatte/scadute, ordina per scadenza; i flag scaduto/oggi/presto fanno il lavoro di sollecito. Un widget della dashboard mostra cosa è aperto.

## Goals {#goals}

Obiettivi con **metriche misurabili**. Dai a ogni obiettivo una scadenza, una categoria, e una o più metriche con un valore attuale, un target e dei punti. Incrementale man mano che procedi, e il completamento dell'obiettivo segue i punti. Filtra aperti/raggiunti/scaduti; trascina per ordinare.

## Calendar {#calendar}

Un calendario personale (anno/mese/settimana/giorno) per eventi con categoria, colore, luogo e note. Il suo punto di forza sono le **sovrapposizioni**: può mostrare anche i tuoi **promemoria**, i **ToDo con una scadenza** e le **scadenze degli obiettivi**, ciascuno attivabile: un posto solo per vedere la settimana. Creare un evento può creare anche un promemoria sincronizzato all'ora di inizio.

## RemindMe {#remindme}

Promemoria, singoli o ricorrenti (con data di inizio, data di fine o numero massimo), che scattano come **notifiche in-app** con una casella delle notifiche.

- **Promemoria collegati**: allega un promemoria a un elemento di un altro modulo (un obiettivo, l'addebito di un abbonamento, una revisione del journal…) e rimanda direttamente a esso. La maggior parte dei moduli ha un pulsante *Aggiungi promemoria* che lo precompila.
- **Canali**: consegna anche a **email, Telegram, Slack o Discord**, scelti dai [canali di notifica](/it/config/settings#notifications) condivisi. Un promemoria elenca i canali concessi a RemindMe; le credenziali e i permessi vivono nelle Impostazioni, una volta per tutta l'app.

## Webhooks {#webhooks}

Dai a qualsiasi servizio esterno un URL privato per **inviare alert con POST a OpenTraderWorld**: piattaforme di grafici e alert, notifiche dei broker, monitor di uptime, script, qualsiasi cosa possa lanciare una richiesta HTTP. Il payload viene ricevuto e instradato in un modulo.

- **URL privato, nessun header**: ogni endpoint porta un **token a 256 bit nel percorso dell'URL** (`/api/hooks/<token>`), perché molti mittenti di alert non possono impostare un header `Authorization`. I token sono salvati con **hash** e mostrati **una sola volta** alla creazione; le ricerche fallite vengono rallentate.
- **Payload tolleranti**: invia testo semplice o JSON; il parser accetta nomi di campo approssimativi, quindi la maggior parte dei mittenti funziona senza formattazione speciale.
- **Instradamento**: ogni endpoint reindirizza il suo payload a un modulo di destinazione. La destinazione della v1 è **[RemindMe](#remindme)**: un payload in arrivo diventa una notifica in-app, inoltrata ai tuoi canali abilitati (email/Telegram/Slack/Discord).
- **Log di consegna**: le consegne più recenti per endpoint vengono conservate così puoi confermare che un mittente ti raggiunge e vedere cosa ha inviato.

Gestisci gli endpoint su **/webhooks**.

::: warning Il mittente deve poterti raggiungere
Un webhook è utile solo se il servizio mittente può aprire una connessione verso il tuo host. In modalità di rete `local` (e LAN semplice) nulla dall'esterno può, e la pagina ti avvisa quando la modalità attuale non è raggiungibile da internet. Cambia la modalità in [Impostazioni → Rete](/it/config/network), oppure punta un tunnel (ad es. Cloudflare Tunnel) all'host e tieni l'app per il resto privata.
:::

## Trading Routines {#routines}

**Checklist di sessione** ricorrenti dovute nei giorni feriali che scegli: preparazione pre-market, disciplina in sessione, revisione post-market. Spunta le voci per giorno, sfoglia i giorni passati e guarda la **striscia di coerenza a 14 giorni** per vedere se rispetti davvero il tuo processo. Sono incluse checklist iniziali.

## Time Tracker {#time}

Progetti con **timer** start/stop (o intervalli aggiunti a mano), **budget di tempo** opzionali con avvisi di superamento, date di fine pianificate e una **tariffa oraria** per valorizzare il tempo. La scheda **Dettaglio** mostra le ore tracciate per giorno/settimana/mese, filtrabili per progetto e categoria. Se un timer è rimasto acceso mentre l'app era chiusa, chiede se mantenere o annullare quel tempo.

## Mindset {#mindset}

Un **check-in** giornaliero per la psiche del trader. Rispondi ad alcune domande prima o dopo la sessione: scale (focus, disciplina), scelte (calmo / ansioso / FOMO), testo libero. Le domande sono **completamente personalizzabili**; è incluso un set iniziale. La vista **Andamenti** mostra le tue risposte negli ultimi check-in, e la Cronologia ti permette di rileggere qualsiasi giorno.

## Prompt Store {#prompt-store}

Una libreria per i **prompt IA** che riutilizzi: riepiloghi di mercato, domande di journaling, modelli di ricerca. I prompt appaiono come una griglia di vignette (nome, tag, ultimo salvataggio) con una ricerca su nome, tag e corpo.

- **Tag**: aggiungi tag liberi nell'editor; filtra la griglia con la barra dei tag.
- **Valuta e filtra**: dai a un prompt un pollice su o giù e filtra rapidamente per uno dei due.
- **Cronologia delle versioni**: ogni salvataggio viene conservato; apri la **Cronologia** di un prompt per vedere in anteprima qualsiasi revisione precedente e **tornare** ad essa (il ripristino viene salvato come nuova versione, quindi nulla va perso).
- **Duplica**: crea un fork di un prompt per ramificare una variante.
