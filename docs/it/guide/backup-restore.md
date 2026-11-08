# Backup e ripristino

Tutto vive in un unico database PostgreSQL, quindi un backup è un solo `pg_dump`. **Impostazioni → Backup e ripristino** nell'app mostra gli stessi comandi. Eseguili sull'host in cui è distribuito lo stack, dalla cartella di installazione (quella che contiene `deploy/`); usano il container Postgres esistente, nessun accesso aggiuntivo necessario.

La sezione ha due schede, ciascuna divisa in **Backup** e **Ripristina**:

- **Completo**: l'intero database, eseguito sull'host, per quando la macchina muore.
- **Parziale**: i moduli che spunti, in un unico zip, per traslocare o tenere una copia leggibile.

## Backup completo

Dump semplice:

```bash
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner > otw-backup-$(date +%F).sql
```

### Cifralo (consigliato)

Un dump contiene i tuoi dati in chiaro. Passalo a `gpg` (o `age`) così il file è cifrato su disco. Ti verrà chiesta una passphrase; `GPG_TTY` e `--pinentry-mode loopback` fanno arrivare quella richiesta al tuo terminale mentre il dump riempie l'input di gpg:

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner \
  | gpg --symmetric --cipher-algo AES256 --pinentry-mode loopback \
  > otw-backup-$(date +%F).sql.gpg
```

### Backup automatico {#automatic-backup}

Quando uno script di backup sull'host riferisce all'app (l'[installer per server](#server-installs) ne configura uno), la scheda **Completo** si apre con una scheda **Backup automatico**: quanto tempo fa è stato fatto l'ultimo backup, dove è andata la copia, se è stato verificato, la pianificazione e la dimensione. Passa ad avviso quando l'ultima esecuzione è fallita, o dopo 48 ore senza backup. Senza uno script del genere la scheda non c'è, e i comandi qui sopra sono tutto ciò che serve.

## Note di sicurezza

- Le **chiavi API e le credenziali dei provider** (feed di notizie, provider di dati di mercato) sono già cifrate a riposo con `OTW_SECRET_KEY`, quindi nel dump compaiono solo come testo cifrato.
- Fai il backup di **`OTW_SECRET_KEY`** (da `deploy/.env`) **separatamente**, non nello stesso dump, altrimenti quei segreti cifrati non si possono ripristinare.
- Il dump include i **token di sessione** attivi. Tratta il file come un segreto, oppure svuota la tabella `sessions` dopo il ripristino (`DELETE FROM sessions;`) e accedi di nuovo. Non eliminare la tabella: l'accesso ne ha bisogno.
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

Un dump si ripristina solo in un database vuoto. Il core esegue le sue migrazioni a ogni avvio, quindi un database su cui è già partito (qualsiasi stack in esecuzione, compreso uno appena creato da `setup.sh`) entra in conflitto con il dump. Ferma il core, sostituisci il database con uno vuoto, carica il dump, riavvia il core:

```bash
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q < otw-backup-YYYY-MM-DD.sql
docker compose -f deploy/docker-compose.yml start core
```

La riga `DROP DATABASE` cancella ciò che lo stack contiene adesso: fanne prima un dump se ti serve. `ON_ERROR_STOP=1` si ferma al primo errore invece di caricare mezzo dump.

Da un backup cifrato:

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
gpg -d otw-backup-YYYY-MM-DD.sql.gpg \
  | docker compose -f deploy/docker-compose.yml exec -T postgres \
    psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q
docker compose -f deploy/docker-compose.yml start core
```

Assicurati che lo stack ripristinato usi la **stessa `OTW_SECRET_KEY`** di quando è stato fatto il backup, altrimenti le credenziali dei provider salvate saranno illeggibili.

## Installazioni su server {#server-installs}

Una macchina configurata con l'[installer per server](/it/guide/hosting/when-it-breaks#installer-options) ha il comando `otw`, che fa tutto quanto sopra al posto tuo:

| Comando | Cosa fa |
|---|---|
| `otw backup` | Fa subito un backup in `/var/backups/otw` e verifica che il file sia completo. |
| `otw restore` | Elenca i backup presenti sulla macchina, chiede quale, e lo rimette dopo che hai digitato `RESTORE`. Prima fa un backup completo dei dati attuali, poi riavvia il core. |

L'installer pianifica anche `otw backup` ogni notte verso le 03:20, ed è questo che accende la scheda [Backup automatico](#automatic-backup). Una copia notturna esclude le due tabelle che si possono riscaricare (il catalogo degli strumenti e le barre storiche); la copia della domenica è completa. Vengono conservate le ultime 10 copie notturne e le ultime 2 complete. Restano sulla macchina: copiale altrove per sopravvivere a un server morto.
