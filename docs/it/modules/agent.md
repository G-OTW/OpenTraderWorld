# Agent

Un **assistente chat IA** integrato: un pannello chat dentro l'app che può anche agire sui tuoi dati OpenTraderWorld. Aprilo dal modulo **Agent**, dalla scorciatoia con le scintille nella barra in alto (accanto alla ricerca), o dal [pulsante flottante](#the-floating-assistant) nell'angolo di ogni pagina.

L'assistente è **bring-your-own-provider**: nulla è abilitato né preimpostato da un vendor finché non aggiungi un provider e una chiave tua.

::: tip Due "agent IA" diversi
Questa pagina riguarda l'**assistente chat che vive nell'app** e parla con un provider che configuri *tu*. Non è la stessa cosa della pagina [Agent IA (MCP)](/it/config/ai-agents), che riguarda gli agent *esterni* che si collegano **a** OpenTraderWorld tramite il server MCP in uscita. L'assistente chat può *usare* quello stesso gateway per raggiungere i tuoi dati: vedi [Strumenti sui tuoi dati](#tools-over-your-data) più sotto.
:::

## Aggiungi un provider

In **Impostazioni** (l'ingranaggio nella barra laterale della chat) → **Generale**, aggiungi uno o più provider. Sono supportati due formati wire:

- **Anthropic**: la Claude Messages API.
- **Compatibile OpenAI**, cioè qualsiasi endpoint che parla il formato chat OpenAI: OpenRouter, OpenAI, DeepSeek, Moonshot, Groq, Mistral, l'endpoint di compatibilità di Gemini, un proxy locale, e così via.

Ogni provider ha il proprio **base URL** (solo compatibile OpenAI), **chiave API** e **modello predefinito**. La chiave è **in sola scrittura**: è cifrata a riposo con la chiave principale dell'app e non viene più mostrata dopo il salvataggio. Lascia vuoto il campo della chiave durante la modifica per mantenere quella attuale.

Un provider può essere disabilitato senza eliminarlo. L'assistente è "pronto" solo quando ha un provider abilitato con una chiave e un modello.

## Configura l'assistente

Nello stesso pannello delle impostazioni imposti il **system prompt**, il **provider / modello** attivo, i **max token** e la **temperatura**. Un campo **Parametri avanzati (JSON)** passa qualsiasi campo di richiesta extra così com'è al provider. Imposta un valore su `null` per *rimuovere* una chiave che l'app altrimenti invierebbe (ad es. `max_completion_tokens` per i modelli OpenAI più recenti, o per eliminare `stream_options`).

## Chat

- Le risposte arrivano **in streaming live** e sono rese in Markdown. I modelli che espongono il ragionamento ottengono un riquadro **Thinking** opzionale.
- Le conversazioni sono salvate nella **barra laterale**: nuova, seleziona, rinomina, elimina, ed **esportazione Markdown** con un clic.
- **Scarica nell'Editor** trasforma una conversazione in una pagina dell'Editor: scegli una cartella (o creane una) e un nome di file, poi **Salva** per restare sull'agent o **Salva e apri**. Ogni messaggio mantiene data e autore (Tu, oppure la persona e il modello), il ragionamento sta in una citazione sopra la risposta, e **Includi dettagli** aggiunge chiamate agli strumenti e conteggi dei token.
- Puoi **fermare** un'esecuzione a metà streaming.
- L'intestazione della chat mostra un **conteggio dei token** progressivo (input + output) per la conversazione, così vedi quanto costa un thread.
- Un interruttore **modalità larga** toglie il limite di larghezza di lettura. Il thread è dimensionato per la prosa, che è la forma sbagliata per le tabelle prodotte dall'assistente: un registro di prove o un dettaglio statistico ha bisogno di spazio. La scelta resta per browser.
- Gli errori del provider si leggono come una frase semplice in un banner chiudibile: una chiave rifiutata, un rate limit (con il retry-after del provider quando lo invia), un modello o base URL sbagliato, un rifiuto del filtro contenuti, o una risposta troncata al limite di token.

### Limiti della chat

Una chat può avere un tetto sui **token di output**, sui **dollari**, o su entrambi. Imposta i valori predefiniti in **Impostazioni → Generale → Limiti delle nuove chat**; ogni nuova chat parte con essi, e cambiarli in seguito lascia in pace le chat esistenti. Lascia un campo vuoto per nessun limite.

La barra sottile accanto al pulsante Prompt Store nella casella dei messaggi si riempie dal basso verso l'alto man mano che la chat spende. Passaci sopra per vedere le cifre, cliccala per cambiare i limiti di questa chat. Quando un limite viene raggiunto, l'esecuzione si ferma prima del suo prossimo passo a pagamento e la chat non accetta nuovi messaggi finché non alzi o rimuovi il limite, cosa che puoi fare dalla stessa barra. Inviare al limite mostra un avviso con due scorciatoie: **Aggiorna limite** apre quell'editor, **Nuova chat** apre una chat nuova con la stessa persona e conserva ciò che hai scritto.

Il limite in dollari richiede un provider che restituisca il prezzo con ogni risposta (OpenRouter sì, Anthropic no). Senza, la spesa appare come "no price returned" e si applica solo il limite di token.

### Cambia provider o modello per chat

L'intestazione della chat mostra il **provider · modello** attivo. Aprirlo ti dà una **schermata di selezione completa**: i provider da un lato, e dall'altro l'**elenco live dei modelli** del provider selezionato, ricercabile, interrogato lato server così la tua chiave non raggiunge mai il browser. Il testo libero funziona ancora per i proxy che non espongono un elenco. Nulla cambia finché non confermi con **Usa questo modello**, quindi sfogliare l'elenco non costa nulla, e un solo pulsante riporta la conversazione al predefinito ereditato.

La scelta appartiene a **quella conversazione**, non all'assistente: un modello economico e veloce può rivedere il tuo journal in una scheda mentre il modello di ragionamento più forte discute di un backtest in un'altra. Una conversazione che non ha fatto una scelta propria eredita quella della persona, poi quella che imposti in **Impostazioni → Generale**. Un puntino sul selettore segna quelle che girano con qualcosa di proprio, e un clic le riporta all'impostazione ereditata. Cambiare provider azzera anche l'id del modello, dato che un nome di modello ha senso solo per il vendor da cui viene.

### Invia un prompt salvato

Il composer può attingere dal tuo [Prompt Store](/it/modules/productivity#prompt-store) invece di riscrivere un prompt che conservi. Il selettore elenca i tuoi prompt con una ricerca su nome, tag e corpo, mostra in anteprima completa quello selezionato e lo inserisce nel composer con **Inserisci prompt** (o doppio clic sulla riga), dove puoi ancora modificarlo prima di inviare.

## L'assistente flottante {#the-floating-assistant}

Un pulsante nell'**angolo in basso a destra di ogni pagina** apre una chat compatta sopra le tue conversazioni esistenti, senza lasciare ciò che stavi facendo. È lo stesso assistente, non uno parallelo: stesse conversazioni, persone, provider e strumenti, quindi un thread iniziato nell'angolo è poi sulla pagina Agent e viceversa. Persona, modello e strumenti stanno su una riga sotto il titolo, e i selettori di modello e prompt si aprono come vista sopra il thread invece che come finestra di dialogo.

Sa anche **su quale pagina sei**. Ogni messaggio porta il modulo corrente, e quando il token della conversazione concede quel modulo la sua lista di endpoint viene caricata nel prompt in anticipo, così una richiesta fatta da Historical Data non spende il primo giro di strumenti a capire dove guardare. Tutto il resto resta a una ricerca di distanza, e una pagina con cui l'assistente non ha nulla a che fare (Impostazioni, la dashboard) non invia nulla.

## Memoria e skill

Due schede nelle impostazioni permettono all'assistente di portare conoscenza tra le conversazioni:

- **Memoria**: fatti piccoli e durevoli (una preferenza, un dettaglio stabile) che persistono tra le chat. Solo l'**indice** (slug + descrizione di una riga) viaggia nel prompt; il contenuto completo viene recuperato su richiesta. Sfogli, modifichi ed elimini ogni memoria tu stesso, nulla è nascosto. Ogni memoria registra **quale persona l'ha scritta**, mostrato sia nel gestore sia nell'indice che l'assistente legge: la memoria è un archivio condiviso, quindi un vincolo scritto dal Day Trader altrimenti sembrerebbe all'Analyst un proprio vincolo. L'assistente può anche sfoltire le memorie da solo quando l'archivio si riempie, e non può sovrascrivere in silenzio una che hai scritto a mano.
- **Skill**: insiemi di istruzioni Markdown riutilizzabili che definisci. **Nome + descrizione** di una skill sono sempre nel contesto; l'assistente carica il corpo completo su richiesta quando un compito lo richiede. Abilita/disabilita ogni skill singolarmente.

Le conversazioni lunghe ottengono anche un **riepilogo progressivo**: quando una chat cresce molto, i turni più vecchi vengono compressi in un riepilogo corrente così il thread resta economico, mantenendo alla lettera solo i messaggi più recenti.

## Persone

Una **persona** è una versione dell'assistente a forma di ruolo: un system prompt con una postura e un confine esplicito di rifiuto, più uno **scaffale di skill** curato. Cinque sono incluse (**Quant**, **Portfolio Manager**, **Day Trader**, **Researcher**, **Financial Analyst**) e ne scegli una quando apri una conversazione, dal selettore di persona nell'intestazione della chat.

L'idea è la specializzazione. Un assistente generico con duecento endpoint è peggiore in ogni singolo lavoro di uno che ne conosce a fondo pochi e rifiuta il resto. Il Quant non riporterà un backtest senza il numero di prove e una cifra out-of-sample; il Day Trader non indicherà un ingresso; l'Analyst riporta i fondamentali che *non è riuscito* a ottenere invece di inventarli.

### Cosa non è una persona

**Una persona non è un insieme di permessi.** Ciò che l'assistente può raggiungere è il token MCP della conversazione: limitato per modulo, impostato da te, identico quale che sia la persona che parla. Cambiare persona restringe la *postura e lo scaffale*, mai l'accesso ai dati. Per cambiare ciò che può toccare, cambia il token.

### Cambiare a metà conversazione

Puoi cambiare persona a metà thread. Ha effetto **dal messaggio successivo**, e un marcatore compare nella trascrizione a registrare il passaggio: i turni sopra di esso sono stati prodotti dalla persona precedente e restano a lei attribuiti.

### Modificare le tue

**Impostazioni → Persone** elenca ogni persona con lo scaffale che otterrà davvero. Da lì puoi:

- **crearne** una da zero, oppure **duplicarne** una inclusa e riscrivere la copia;
- modificare il **prompt**, lo **scaffale**, e se **approva automaticamente le scritture**;
- **ripristinare** una integrata a come è distribuita (le tue modifiche vanno perse, nient'altro viene toccato);
- **eliminarne** una che hai creato. Le sue conversazioni vengono **conservate**: passano all'assistente predefinito, e ogni trascrizione riceve una nota che lo dice. Le integrate non si possono eliminare: l'app le ricrea al riavvio successivo, quindi l'eliminazione sembrerebbe solo funzionare.

Le skill **non** si creano qui. C'è un unico catalogo, gestito nella scheda Skill, e le persone scelgono da esso. Questo significa che modificare il corpo di una skill la cambia per ogni persona che la possiede, e l'elenco delle skill mostra quante, quindi la modifica non è mai alla cieca.

### Esporta e importa

Qualsiasi persona si esporta come **file JSON con i corpi delle skill inline**, così un file la riproduce su un'altra macchina. Puoi anche esportare una singola skill, o l'intero scaffale in una volta. L'import accetta entrambe le forme; un nome esistente viene saltato invece di sovrascritto. Non c'è un controllo di revisione: è la tua macchina, e ciò che carichi nel tuo assistente è una tua scelta.

La gestione di persone e skill è deliberatamente **assente dal catalogo MCP**: nessun agent, e nessun contenuto letto da un agent, può modificare una persona o ampliare uno scaffale.

## Conferma delle scritture

Quando l'assistente vuole modificare i tuoi dati, l'esecuzione va in **pausa** e ti mostra la chiamata esatta (metodo, percorso e body) con Approva e Rifiuta. Nulla viene scritto finché non rispondi, e il rifiuto viene riportato al modello come un diniego e non come un errore da aggirare. Se ti allontani, l'attesa scade e la scrittura non avviene.

Una persona può essere impostata per **approvare automaticamente le scritture**, il che salta la richiesta per le modifiche ordinarie. **Le eliminazioni chiedono sempre**, qualunque cosa dica quell'impostazione: spuntare la casella era una decisione sulle scritture di routine, non il permesso di cancellare un journal.

Gli endpoint che *calcolano* soltanto (un backtest, uno Sharpe ratio, una Monte-Carlo) non chiedono. Non cambiano nulla di cui sentiresti la mancanza, e una finestra di conferma a ogni calcolo è il modo in cui si impara a cliccare Approva senza leggere.

## Strumenti sui tuoi dati {#tools-over-your-data}

Collega un **token MCP** a una conversazione e l'assistente può leggere e aggiornare i tuoi moduli tramite lo [stesso gateway in-process](/it/config/ai-agents) usato dai client MCP esterni. I **livelli di permesso per modulo** del token (Lettura / Lettura+scrittura / Completo, impostati in **Impostazioni → MCP**) si applicano **direttamente**: il token *è* l'involucro dei permessi; non c'è un secondo controllo lato agent. Impostazioni, segreti, rete e operazioni di cancellazione dati non sono mai esposti, e non c'è accesso a shell o filesystem per costruzione.

Le chiamate agli strumenti compaiono inline come **chip comprimibili** che mostrano argomenti e risultato. Un'esecuzione è limitata a 15 giri di strumenti, e ogni conversazione porta un **budget di simulazione**, dato che backtest e sweep sono l'unica cosa che un assistente può spendere senza limite, quindi una volta esaurito il budget gli viene detto di smettere di cercare e riportare ciò che ha, compreso quante prove ha eseguito.

### Scrivere la tua skill

Una skill è una procedura, non un manuale. La forma che funziona:

- una **descrizione** che dice *quando* ricorrervi, dato che quella riga è la chiave di recupero e viaggia in ogni prompt, quindi "Use when the user proposes a strategy" batte "About strategies";
- un **corpo** che nomina gli endpoint esatti, passo per passo, con i modi specifici in cui il compito va storto in questa app;
- un passo di **verifica**: come provare il risultato prima di riportarlo;
- una **forma del report**: cosa deve contenere la risposta.

Mantieni il corpo breve. Finisce intero nella finestra di contesto quando viene caricato, quindi uno lungo soffoca il compito che doveva aiutare. L'editor ti avvisa oltre circa duemila parole.

### Strumenti per conversazione

Ogni conversazione porta **il proprio** token MCP (il token che imposti nelle impostazioni è solo il predefinito per le nuove conversazioni), cambiabile da un **menu a tendina degli strumenti** nell'intestazione della chat. Due conversazioni possono girare con ambiti di dati diversi affiancate. Il menu:

- ha una **casella di ricerca** per filtrare token e server esterni per nome;
- segnala quando il token selezionato concede **scrittura/eliminazione**;
- offre azioni inline per **aggiungere un server MCP** e collegamenti rapidi a **Impostazioni → MCP** (crea/gestisci token) e al **MCP store**.

## MCP store: collega piattaforme esterne

**Agent → Gestisci server** è una sezione a pagina intera per aggiungere server MCP remoti così l'assistente può raggiungere piattaforme esterne:

- un **catalogo curato** di server noti (DeepWiki, Context7, GitHub, Hugging Face, porta la tua chiave), più **server personalizzati** per URL;
- **solo Streamable-HTTP**: nulla viene mai eseguito in locale;
- i valori di autenticazione sono **cifrati a riposo e in sola scrittura**;
- un pulsante **Test** si connette ed elenca gli strumenti del server;
- abilita un server per conversazione dal menu degli strumenti.

Gli strumenti esterni hanno **namespace** (ad es. `deepwiki__ask_question`) e sono etichettati con il loro server, le chiamate hanno un tempo limite, e un server irraggiungibile **degrada a un avviso** invece di bloccare la chat.

::: warning I contenuti esterni non sono affidabili
Un server MCP esterno vede la tua conversazione, e ciò che restituisce è contenuto di terze parti. Combinare un server esterno con un token che concede **accesso in scrittura** ai tuoi dati significa che contenuto iniettato potrebbe tentare di innescare modifiche, e il menu degli strumenti ti avvisa quando quella combinazione è attiva. Aggiungi solo server di cui ti fidi, e tieni d'occhio i chip delle chiamate agli strumenti.
:::
