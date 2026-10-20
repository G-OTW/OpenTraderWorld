# Backup e ripristino

Tutto vive in un unico database PostgreSQL, quindi un backup è un solo `pg_dump`. **Impostazioni → Backup e ripristino** nell'app mostra questi comandi già compilati per il tuo deployment. Eseguili sull'host in cui è distribuito lo stack; usano il container Postgres esistente, nessun accesso aggiuntivo necessario.

La sezione ha due schede, ciascuna divisa in **Backup** e **Ripristina**:

- **Completo**: l'intero database, eseguito sull'host, per quando la macchina muore.
- **Parziale**: i moduli che spunti, in un unico zip, per traslocare o tenere una copia leggibile.

## Backup completo

Dump semplice:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld > otw-backup-$(date +%F).sql
```

### Cifralo (consigliato)

Un dump contiene i tuoi dati in chiaro. Passalo a `gpg` (o `age`) così il file è cifrato su disco. Ti verrà chiesta una passphrase:

```bash
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld | gpg -c --cipher-algo AES256 -o otw-backup-$(date +%F).sql.gpg
```

## Note di sicurezza

- Le **chiavi API e le credenziali dei provider** (feed di notizie, provider di dati di mercato) sono già cifrate a riposo con `OTW_SECRET_KEY`, quindi nel dump compaiono solo come testo cifrato.
- Fai il backup di **`OTW_SECRET_KEY`** (da `deploy/.env`) **separatamente**, non nello stesso dump, altrimenti quei segreti cifrati non si possono ripristinare.
- Il dump include i **token di sessione** attivi. Tratta il file come un segreto, oppure elimina la tabella `sessions` dopo il ripristino e accedi di nuovo.
- Conserva il backup cifrato **fuori dalla macchina** e ruota le copie più vecchie.

## Parziale (per modulo)

La scheda **Parziale** prende i moduli che spunti e ti dà **un unico file zip**, per spostare un journal su un'altra istanza o tenere una copia leggibile. Funziona con l'app attiva, a differenza del backup completo.

### Backup parziale

1. Apri **Impostazioni → Backup e ripristino → Parziale → Backup**.
2. Spunta i moduli che vuoi. Ognuno mostra il numero di righe e la dimensione, il totale dell'insieme spuntato appare sotto l'elenco, e *Contenuto della selezione* lo scompone tabella per tabella. Le barre storiche partono non spuntate: sono di gran lunga la tabella più grande e si possono riscaricare dal tuo provider.
3. Lascia disattivato **Includi le credenziali dei provider salvate** a meno che tu non sappia perché ti servono. Quei valori sono cifrati con la `OTW_SECRET_KEY` di questa istanza e sono illeggibili altrove.
4. Clicca **Scarica i dati selezionati**. Ottieni `otw-data-YYYY-MM-DD.zip`.

Dentro lo zip: `manifest.json` (cosa contiene, quale versione l'ha scritto) e un `tables/<name>.jsonl` per tabella, un oggetto JSON per riga. Qualsiasi strumento lo può leggere.

### Ripristino parziale

1. Apri **Impostazioni → Backup e ripristino → Parziale → Ripristina** sull'istanza di destinazione e scegli il file.
2. Il file viene letto appena lo scegli, e nulla viene scritto: ottieni la versione che lo ha scritto e, per modulo e per tabella, quante righe contiene rispetto a quante ce ne sono ora. Una tabella molto grande viene riportata come stima, segnata con `~`. Un file che questa istanza rifiuterebbe (danneggiato, o di una release più recente) viene rifiutato a questo punto, prima che tu ti impegni in qualsiasi cosa.
3. Scegli come deve incontrare i dati già presenti:
   - **Aggiungi ciò che manca** mantiene tutto ciò che c'è e aggiunge solo le righe non ancora presenti. Nulla viene sovrascritto.
   - **Sostituisci** cancella i dati di ogni modulo nel file, poi carica la versione del file. Ti chiede di digitare `REPLACE`, e prima salva una copia dei dati attuali.

   La riga sotto la scelta traduce quei conteggi in ciò che accadrà. Sostituisci è esatto: *cancella le N righe presenti, mette al loro posto le M righe del file*. Unisci può dare solo un massimo, *aggiunge fino a M righe*: una riga la cui chiave è già presente viene saltata, e solo il caricamento stesso sa quante sono.
4. Clicca **Carica questo file**.

Tutto avviene in un'unica transazione: se una qualsiasi parte fallisce, nulla viene modificato.

::: warning Un file di una versione più recente viene rifiutato
Caricare un bundle scritto da una release più recente viene rifiutato invece di essere tentato. Aggiorna prima l'istanza, poi caricalo.
:::

Le righe mantengono i loro identificatori originali e l'intero file viene letto in memoria, quindi una selezione molto grande (tipicamente le barre storiche) viene rifiutata con un messaggio che rimanda al backup `pg_dump` qui sopra. È lo strumento giusto per "tutto, compreso ciò che non guardo mai".

## Ripristino completo

In un database nuovo e vuoto (uno stack appena creato):

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld < otw-backup-2026-07-06.sql
```

Da un backup cifrato:

```bash
gpg -d otw-backup-2026-07-06.sql.gpg | \
  docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld
```

Assicurati che lo stack ripristinato usi la **stessa `OTW_SECRET_KEY`** di quando è stato fatto il backup, altrimenti le credenziali dei provider salvate saranno illeggibili.
