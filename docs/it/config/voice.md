# Controllo vocale

Guida OpenTraderWorld con la voce, o detta in qualsiasi campo di testo. **Solo push to talk**: il microfono si apre quando premi e si chiude quando rilasci. Nulla ascolta nel frattempo.

**È disattivato per impostazione predefinita.** Attivalo in **Impostazioni → Voce → Voce e scorciatoia**.

## Il microfono richiede HTTPS o localhost {#https}

I browser concedono a una pagina web il microfono (e il loro riconoscimento vocale integrato) solo su un'**origine sicura**: una pagina servita in **HTTPS**, oppure aperta su **localhost**. È una regola del browser, non di OpenTraderWorld, e nessuna impostazione dell'app può toglierla.

| Come apri OTW | La voce funziona? |
|---|---|
| Sulla macchina che lo esegue, `http://127.0.0.1:5454` o `http://localhost:5454` | Sì |
| Modalità [LAN + HTTPS](/it/config/network#lan-https) o [Pubblico](/it/config/network#public) | Sì |
| Modalità [Rete locale (LAN)](/it/config/network) in HTTP semplice, da un altro dispositivo | **No**, il browser nasconde il microfono |

Per usare la voce da un telefono o da un altro computer della tua rete, porta **Impostazioni → Rete** su **LAN + HTTPS**. In HTTP semplice la pagina delle impostazioni vocali mostra un avviso e il pulsante del microfono spiega perché non può partire.

## Due scorciatoie, due modalità

| | Predefinita | Cosa succede a ciò che dici |
|---|---|---|
| **Comando** | `Alt+V` (`⌥V` su Mac) | Diventa un piano di azioni, anche quando un campo di testo ha il focus. |
| **Dettatura** | `Alt+Shift+V` (`⌥⇧V`) | Viene digitata, parola per parola, nel campo di testo che ha il focus. Mai interpretata come comando. |

Tieni premuta la scorciatoia mentre parli, rilascia per finire, `Esc` per annullare. Il **microfono nella barra in alto** esegue sempre comandi: clicca per iniziare, clicca di nuovo per fermare, oppure tienilo premuto come un walkie-talkie.

Entrambe le scorciatoie si possono cambiare in **Impostazioni → Voce → Voce e scorciatoia**. Ciascuna richiede `Ctrl`, `Alt` o `⌘` (oppure un tasto funzione da `F1` a `F12`), così non intralcia mai la scrittura normale, e le due devono essere diverse.

## Motore di riconoscimento

Il motore trasforma la tua registrazione in testo. Scegline uno in **Impostazioni → Voce → Voce e scorciatoia**.

| Motore | Configurazione | Dove va l'audio |
|---|---|---|
| **Questo browser** | nessuna | Il servizio vocale del produttore del browser (Google per Chrome, Microsoft per Edge, Apple per Safari). Non disponibile in Firefox. |
| **Whisper self-hosted** | il servizio incluso, vedi [Esempio: Whisper self-hosted](#whisper-example) | Resta sulla tua macchina |
| **Server whisper.cpp** | il tuo server, il suo endpoint `/inference` | Resta sul tuo server |
| **OpenAI / Groq** | una chiave API (incollata o collegata dalla [Vault](/it/config/settings#vault)) | Inviato a quel provider |
| **Altro** | qualsiasi server che esponga l'API OpenAI `/audio/transcriptions` | Quel server |

Con un motore server il browser registra, converte l'audio in un piccolo file WAV e OTW lo inoltra al motore. **Testa** invia mezzo secondo di silenzio per verificare URL, chiave e modello. Le chiavi sono cifrate a riposo e mai rimandate al browser.

### Esempio: Whisper self-hosted, da zero {#whisper-example}

Il servizio Whisper incluso è opzionale e non viene avviato da un semplice `up`. I passi da 1 a 3 si fanno una volta sola: il modello resta nel volume `whisper-cache` tra i riavvii.

**1. Avvia il servizio**, dalla radice del repository:

```bash
docker compose -f deploy/docker-compose.yml --env-file deploy/.env --env-file deploy/network.env --profile voice up -d whisper
docker logs -f opentraderworld-whisper-1
```

Aspetta `Uvicorn running on http://0.0.0.0:8000`, poi `Ctrl+C`. La porta è raggiungibile solo da OTW dentro Docker, non dal tuo browser, ed è voluto.

**2. Scarica il modello** (circa 500 MB, qualche minuto):

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen(urllib.request.Request('http://localhost:8000/v1/models/Systran/faster-whisper-small',method='POST'),timeout=1800).read())"
```

**3. Verifica che sia installato**: la risposta deve elencare `Systran/faster-whisper-small`.

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen('http://localhost:8000/v1/models').read())"
```

**4. Collegalo a OTW.** Apri l'app su `http://localhost:5454` (o in HTTPS, vedi [sopra](#https)), poi **Impostazioni → Voce → Voce e scorciatoia**:

1. **Aggiungi motore**, preset **Whisper self-hosted**: compila `http://whisper:8000/v1` e `Systran/faster-whisper-small`.
2. **Salva e testa**: il motore mostra **Funziona · … ms**.
3. Seleziona il motore, poi porta l'interruttore in alto su **Attivo**. Se vuoi, imposta la **Lingua**.

**5. Provalo**: tieni premuto `⌥V` / `Alt+V`, di' *"open journal"*, rilascia, premi `Invio`. Per la dettatura, clicca in un campo di testo e tieni premuto `⌥⇧V` / `Alt+Shift+V`.

La prima trascrizione dopo un riavvio è più lenta mentre il modello si carica in memoria.

## Comandi vocali

**Impostazioni → Voce → Comandi vocali**: un comando è una **frase** (più altri modi di dirla) e un elenco ordinato di **passi**:

- **Apri una pagina**: un modulo, la dashboard o le Impostazioni.
- **Chiedi all'agent**: un prompt inviato all'assistente flottante, che agisce con i propri strumenti.
- **Esegui un flusso**: avvia un flusso [Automator](/it/modules/automator).
- **Tema**, **Nascondi le cifre**: come i pulsanti nella barra in alto.
- **Parla**: legge una frase ad alta voce.

Una frase scatta solo quando viene detta **da sola**, mai come parola dentro una frase: dettare "a blue turtle" non esegue il tuo comando *turtle*.

### Integrati, senza configurazione

- **"open &lt;page&gt;"** (*"ouvre &lt;page&gt;"*, *"öffne &lt;Seite&gt;"*, *"abre &lt;página&gt;"*, *"apri &lt;pagina&gt;"*, *"打开 &lt;页面&gt;"*) apre qualsiasi modulo installato, la dashboard o le Impostazioni. Un nome che corrisponde a più pagine viene rifiutato con l'elenco, mai indovinato.
- Con **Passa il resto all'agent** attivo, tutto ciò che nessun comando riconosce va all'assistente come un'unica richiesta.

### Concatenazione

Di' più cose in una volta, unite da *and* o *then* (*e*, *poi* in italiano, e così via, secondo l'impostazione **Lingua**). Anche una virgola nella trascrizione divide:

> "turtle, then open settings and compare AAPL and MSFT"

esegue il tuo comando *turtle*, apre le Impostazioni, poi chiede all'agent di "compare AAPL and MSFT". I frammenti rimasti uno accanto all'altro restano uniti, così l'agent riceve l'intera richiesta.

## Conferma

Ogni piano è **mostrato prima di essere eseguito**: cosa è stato sentito, ogni passo e da dove viene. `Invio` lo esegue, `Esc` lo annulla. I passi si eseguono in ordine e il piano si ferma al primo errore, indicando il passo.

Un comando può essere contrassegnato **Esegui senza conferma**. È **disattivato per impostazione predefinita**, e vale solo quando quella frase è detta da sola: concatenata con qualsiasi altra cosa, il piano viene comunque mostrato prima. I passi dell'agent mantengono la conferma propria dell'assistente per ogni scrittura.

## Da sapere

- L'assistente è nascosto nella pagina **Agent**, quindi un piano con un passo dell'agent non può essere eseguito da lì.
- **Leggi l'esito ad alta voce** usa le voci del browser: nessuna configurazione, nessun audio lascia il browser.
- La demo pubblica mostra le pagine vocali in sola lettura: non salva le impostazioni né invia audio.
- **Il tuo reverse proxy** davanti a OTW non deve bloccare il microfono: il suo header `Permissions-Policy` deve contenere `microphone=(self)`. Con `microphone=()` il browser rifiuta subito, senza chiedere.
