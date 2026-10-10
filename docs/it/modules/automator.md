# Automator

*Workflow che fanno girare la tua app.* Un workflow è un elenco di passi: chiama un endpoint della tua API OpenTraderWorld, chiama qualsiasi URL esterno, fai una domanda al tuo provider IA, rimodella la risposta, notifica te stesso. Eseguilo a mano, o con una pianificazione.

Usi tipici: un brief pre-market del lunedì che legge gli eventi economici della settimana e le tue watchlist e invia un unico messaggio Telegram; un export notturno del journal verso un servizio esterno; un webhook di prezzo smistato in una notifica; un riepilogo settimanale scritto dall'assistente dai tuoi numeri.

Il modulo vive su **/automator** con quattro sezioni: **Workflow**, **Pianificazioni**, **Agenda** (un calendario delle esecuzioni a venire), **Esecuzioni**.

## La griglia

L'editor è una griglia, non una tela: nessun filo da tracciare.

- **I passi vanno dall'alto in basso, i task dentro un passo da sinistra a destra.** Tutto viene eseguito uno dopo l'altro. Un passo raggruppa i task che appartengono allo stesso momento del workflow, non li avvia insieme.
- Trascina un blocco dalla palette su uno **spazio**: lo spazio tra due passi apre un nuovo passo, lo spazio tra due task lo inserisce in quel passo. Ogni destinazione valida è evidenziata prima del rilascio.
- Un task può leggere solo ciò che è girato **prima** di lui. Sposta una scheda e i suoi riferimenti vengono ricontrollati al salvataggio successivo.
- **Salvataggio automatico** 1,2 s dopo ogni modifica, **Ctrl/Cmd+Z** per annullare. Un blocco ancora in compilazione è tenuto come **bozza**: non gira mai e nessuna pianificazione lo prende finché non è valido.

## Blocchi

| Blocco | Cosa fa |
|---|---|
| **App call** | Un endpoint della tua API, eseguito in-process. La palette elenca gli endpoint che un workflow può raggiungere; cliccane uno e il blocco arriva già puntato su di esso, con la forma attesa del body a portata di mano. |
| **Web call** | Qualsiasi URL esterno all'app: metodo, query, header, body JSON/testo/form, risposta letta come JSON, testo, CSV o binario, con un limite di dimensione della risposta. |
| **AI step** | Un turno di modello, senza conversazione intorno. Scegli una persona e un prompt salvato o scrivi le istruzioni; chiedi un **oggetto JSON** quando il blocco successivo deve leggere campi invece di prosa. Chiama il tuo provider e costa token a ogni esecuzione. |
| **Transform** | Rimodella ciò che è venuto prima: `pick` un valore, `set` un oggetto, `format` una stringa, `csv_parse` un CSV, `join` un elenco in una riga. |
| **Notify** | Una notifica in-app e i tuoi [canali di notifica](/it/config/settings#notifications) (email, Telegram, Slack, Discord). Nessuna selezione significa ogni canale abilitato concesso all'Automator. |
| **Wait** | Mette in pausa l'esecuzione. **Stop** risponde comunque durante l'attesa. |

## Passare dati tra i blocchi

Ogni blocco ha un **id**, mostrato in cima al suo editor. Un blocco successivo legge il suo risultato con un'espressione:

```
{{steps.http1.output}}                     the whole answer
{{steps.http1.output.items.0.name}}        one field of it
{{steps.http1.status}}                     ok | simulated | failed | skipped
{{steps.http1.error}}                      the failure message, empty on success
{{run.started_at}} {{run.trigger}} {{workflow.name}} {{input.key}}
```

I filtri si concatenano dopo una pipe: `json`, `upper`, `lower`, `trim`, `round:2`, `date:"DD/MM/YYYY"`, `default:"n/a"`. Solo `default` salva un valore che non c'è.

Questo **non è un linguaggio**: niente aritmetica, niente codice. Ogni riferimento è controllato **al salvataggio del grafo**, rispetto ai blocchi che davvero lo precedono, quindi un collegamento rotto viene rifiutato nell'editor invece che alle tre di notte.

## Segreti

Una password o una chiave API appartiene al [Vault](/it/config/settings#vault), mai digitata in un campo. Referenziala con <code v-pre>{{vault.myvault.mykey}}</code> in un header, in un valore di query o nel body di una richiesta, gli unici posti in cui è accettata (mai in un URL), e il valore risolto viene ripulito dalla cronologia delle esecuzioni. Un valore di query raggiunge comunque i log del sito che chiami, quindi preferisci un header.

## Permessi

- **App call richiede un token di accesso.** Sceglilo nelle **Impostazioni** del workflow; i token si creano in [Impostazioni → Agent IA](/it/config/ai-agents). Nessun token significa nessuna chiamata interna, e un workflow può raggiungere solo ciò che il suo token concede, entro la stessa allowlist del gateway MCP.
- **Web call rifiuta la tua rete.** Gli indirizzi loopback, privati, CGNAT e link-local sono rifiutati a meno che il blocco consenta esplicitamente destinazioni interne. L'host viene risolto prima e la connessione è vincolata all'indirizzo controllato, e ogni salto di redirect viene ricontrollato.
- **Notify richiede un permesso.** Un canale si propone qui solo dopo che l'Automator ne ha ricevuto la concessione in Impostazioni → Notifiche.

## Provarlo, poi eseguirlo

- **Test run** esegue le letture e riporta cosa una scrittura o un invio *avrebbe* fatto, quindi nulla esce dall'app. Un blocco che legge un valore che solo una scrittura simulata avrebbe potuto produrre è a sua volta riportato come **simulated**, invece di far fallire un test che un'esecuzione reale supererebbe.
- **Testa questo blocco** esegue un blocco da solo, stesse regole.
- **Esegui ora** lo fa davvero. **Stop** viene controllato tra i blocchi e durante un'attesa; un'esecuzione oltre il **limite di esecuzione** del workflow viene chiusa come timeout.

## Esecuzioni

Ogni esecuzione conserva **una riga per blocco**: cosa è stato inviato, cosa è tornato, lo stato e i tempi, così un workflow fallito alle 3 di notte indica il blocco e mostra il payload. Un output oltre 256 KB viene messo da parte e recuperato su richiesta. La cronologia è ridotta alle ultime 50 esecuzioni per workflow (10 per i test run).

## Pianificazioni

Ogni N minuti, ogni giorno, alcuni giorni della settimana, ogni mese, o una volta in un dato momento, ciascuna con il proprio **fuso IANA**, così una regola impostata su Europe/Paris segue Parigi e non il server.

- **Nessuna sovrapposizione**: se un'esecuzione è ancora in corso quando scade l'occorrenza successiva, quell'occorrenza viene **scartata**, non messa in coda.
- **Recupero** (opzionale): se l'app era spenta all'ora prevista, esegui una volta all'avvio.
- L'ora legale è gestita, non ignorata: un'ora locale saltata gira alla fine del salto, una raddoppiata gira una volta. Una regola mensile impostata oltre la fine di un mese corto gira nel suo ultimo giorno.
- Una pianificazione può essere **fissata a una versione** del workflow. Salvare un nuovo grafo chiede se le pianificazioni fissate debbano seguirlo; il salvataggio automatico non ne riposiziona mai una da solo.
- **Pausa / riprendi** dalla scheda del workflow o dall'elenco Pianificazioni. Un workflow disabilitato non viene mai avviato da una pianificazione, ma eseguirlo a mano funziona comunque.

::: warning Una scrittura pianificata scrive
Una App call puntata su un endpoint che modifica i tuoi dati lo farà a ogni esecuzione, senza supervisione. L'editor segnala quegli endpoint; testa il workflow prima di pianificarlo.
:::

## Lasciare che un agent costruisca un workflow {#letting-an-agent-build-a-workflow}

Comporre un grafo è la cosa più difficile che questo modulo ti chiede, ed è esattamente il tipo
di lavoro in cui un [agent IA](/it/config/ai-agents) è bravo. Quindi un agent con un token con il
permesso **Automator** può leggere i tuoi workflow, crearne uno, scriverne il grafo e testarlo.

Ciò che non può fare è metterne uno in servizio. La divisione è deliberata:

- Un grafo salvato da un agent arriva come **bozza**, mai come il grafo che gira. Apri il
  workflow, leggi cosa ha scritto, e Salva per adottarlo. Finché non lo fai, nulla cambia: un
  workflow già in pianificazione continua a eseguire la versione che hai salvato.
- Un agent non può collegare il **token di accesso** di un workflow. Quell'involucro è ciò che trasforma un grafo
  in permessi, quindi lo concedi a mano, dopo aver letto il grafo che eseguirà.
- Un agent non può **eseguire** un workflow, ripristinare una revisione, eliminarne uno, né toccare una pianificazione.
- I suoi **test run** sono sigillati: notifiche e chiamate esterne sono forzate a off, e senza
  involucro collegato una App call fallisce in modo chiuso. Un test verifica che il grafo giri, che le sue
  espressioni si risolvano e che le sue transform facciano ciò che dichiarano, senza raggiungere nulla. Un
  workflow che porta già un token viene rifiutato: quello lo testi tu.

Il motivo della linea è che un workflow gira con **il proprio** token, non con quello del chiamante.
Un agent capace sia di scrivere un grafo sia di avviarlo erediterebbe tutto ciò che quel token concede,
qualunque cosa dicessero i suoi permessi. Scrivere e armare sono due permessi, e solo uno dei due
è tuo da delegare.

L'Automator è disabilitato in [modalità demo](/it/guide/demo).
