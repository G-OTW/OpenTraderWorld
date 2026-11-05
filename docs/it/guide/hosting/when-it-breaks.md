# Quando qualcosa si rompe

I problemi in cui ci si imbatte davvero, nell'ordine in cui di solito capitano.
Per ciascuno: cosa vedi, perché, e cosa fare.

::: tip La prima cosa da provare, sempre
Se l'installazione si è interrotta, **riesegui la stessa riga**.
Ricorda cosa ha già funzionato e riparte da dove si era fermata.
Non pone mai la stessa domanda due volte.
:::

## Durante l'installazione

### "The name … does not lead anywhere yet"

**Perché:** il tuo indirizzo (per esempio `app.example.com`) non è ancora collegato alla tua macchina.

1. Accedi all'azienda da cui hai comprato l'indirizzo.
2. Apri la sua pagina **DNS** (a volte chiamata **Zone** o **DNS records**).
3. Aggiungi un record esattamente con ciò che il messaggio ha stampato:

   | Tipo | Nome | Valore |
   |---|---|---|
   | `A` | la parola mostrata dal messaggio (`@` per l'indirizzo nudo) | i numeri mostrati dal messaggio |

4. Salva.
5. Aspetta cinque minuti.
6. Riesegui la stessa riga.

::: details Il campo "Nome" è l'errore più comune
Per `example.com`, scrivi `@` (alcuni siti vogliono il campo vuoto).
Per `app.example.com`, scrivi solo `app`, non l'indirizzo completo.
:::

### "The name … does not lead to this machine"

**Perché:** l'indirizzo punta altrove: una vecchia macchina, o una pagina di parcheggio dell'azienda che lo ha venduto.

1. Apri la stessa pagina **DNS**.
2. Elimina ogni altro record `A` con quel nome.
3. Tieni solo quello con il valore stampato dal messaggio.
4. Aspetta cinque minuti, poi riesegui la stessa riga.

Se lo hai cambiato solo da pochi minuti, rispondi **sì** quando l'installer offre di aspettare.
Controlla ogni 20 secondi per un massimo di 10 minuti.

### "The address https://… is not answering yet"

**Perché:** quasi sempre una di due cose.

- Hai puntato l'indirizzo alla macchina solo da pochi minuti. **Aspetta dieci minuti**, poi riesegui la stessa riga.
- Il tuo provider ha un proprio firewall davanti alla macchina, ed è chiuso.

Per aprire il firewall del provider:

1. Apri il pannello di controllo del tuo provider.
2. Trova le impostazioni **Firewall** o **Security** della macchina.
3. Consenti in ingresso **TCP 80** e **TCP 443**.
4. Riesegui la stessa riga.

### "Something on this machine is already answering on port 80 / 443"

**Perché:** il tuo provider ha installato per te un web server sulla macchina. Occupa il posto che serve a OpenTraderWorld.

Incolla questo, poi riesegui la stessa riga:

```bash
systemctl disable --now apache2 nginx caddy 2>/dev/null; true
```

### "This machine has … MB of memory" o "Only … MB of disk space is free"

**Perché:** la macchina è troppo piccola. OpenTraderWorld richiede circa **2 GB di memoria** e **8 GB di disco libero**.

1. Presso il tuo provider, ridimensiona la macchina a un piano più grande.
2. Riesegui la stessa riga.

### "This installer only knows Ubuntu and Debian"

**Perché:** la macchina è stata creata con un altro sistema.

1. Presso il tuo provider, **reinstalla** (o **ricostruisci**) la macchina con **Ubuntu 24.04** o **Debian 13**.
2. Riesegui la stessa riga.

### "This needs the machine's administrator rights"

**Perché:** hai effettuato l'accesso con un account che non può installare software.

Riesegui la riga con `sudo` in mezzo, esattamente come mostra il messaggio:

```bash
curl -fsSL https://get.opentraderworld.com/configure_install.sh | sudo bash -s -- --domain app.example.com
```

## Dopo l'installazione

### La pagina non si apre più

1. Accedi alla tua macchina.
2. Digita:

   ```bash
   otw status
   ```

3. Se dice **Nothing is running** o **is not answering**, digita:

   ```bash
   otw restart
   ```

4. Aspetta un minuto, poi ricarica la pagina.

### Non riesco più ad accedere alla macchina stessa

**Perché:** l'installer ha irrigidito il modo in cui la macchina fa entrare le persone.

- Se hai scelto **la chiave**: accedi dallo stesso computer che hai usato il giorno dell'installazione. Le password sono rifiutate di proposito.
- Se hai scelto **una password**: accedi con il nome account mostrato sulla tua scheda, **non** `root`. L'accesso diretto come `root` è rifiutato di proposito.

Hai perso quel computer o quella password? Usa la **console** (a volte chiamata **VNC**, **rescue** o **web terminal**) nel pannello di controllo del tuo provider. Funziona anche quando la via normale d'accesso è chiusa.

### Ho dimenticato la password di OpenTraderWorld

1. Accedi alla tua macchina.
2. Digita (sostituisci `admin` con il tuo nome di accesso se l'hai cambiato):

   ```bash
   docker exec -it opentraderworld-core-1 /app/otw-core reset-password admin
   ```

3. Accedi all'app con la password che stampa. L'app ti chiede di sceglierne una nuova.

Per rivedere il tuo indirizzo e il nome di accesso, digita `otw card`.

### Il browser mostra "Non sicuro" o rifiuta la pagina

- Digita l'indirizzo con `https://` davanti.
- Se è iniziato pochi minuti dopo l'installazione, aspetta dieci minuti: il lucchetto è ancora in fase di emissione.
- In un'installazione domestica (non un server in affitto), vedi [Risoluzione dei problemi](/it/guide/troubleshooting).

### La macchina è piena

**Perché:** i backup notturni e lo storico dei prezzi scaricato occupano spazio nel tempo.

1. Digita `otw status` per vedere quanto spazio resta.
2. Presso il tuo provider, dai alla macchina un disco più grande.
3. Digita `otw restart`.

## Il comando otw {#otw-command}

Tutto ciò che fai sulla macchina dopo il giorno dell'installazione passa da `otw`.

| Digita | Cosa fa |
|---|---|
| `otw status` | Dice cosa è in esecuzione, se il tuo indirizzo risponde, quando è stato fatto l'ultimo backup e quanto disco resta. |
| `otw restart` | Riavvia tutto. |
| `otw backup` | Fa subito un backup e lo verifica. Ne viene fatto uno anche ogni notte verso le 03:20. Vedi [Backup e ripristino](/it/guide/backup-restore#server-installs). |
| `otw restore` | Mostra i backup presenti sulla macchina e rimette quello che scegli. Prima salva ciò che c'è adesso. |
| `otw update` | Aggiorna alla versione più recente. Prima fa un backup, e torna alla versione precedente se la nuova non risponde. Vedi [Aggiornamento](/it/guide/updating#server-install). |
| `otw logs` | Mostra le ultime 200 righe di ciò che ha fatto l'applicazione. `otw logs 1000` ne mostra di più. |
| `otw card` | Mostra il tuo indirizzo, il tuo nome di accesso e dove si trova la password. |
| `otw report` | Scrive un file da allegare a una issue. Non contiene password. |

## Opzioni dell'installer {#installer-options}

Puoi dare all'installer le sue risposte sulla riga, dopo `--domain`. Ciò che ometti, lo chiede. Quando lo esegui di nuovo, tiene le risposte precedenti, e un'opzione che ridai prevale.

| Opzione | Cosa imposta |
|---|---|
| `--domain <name>` | L'indirizzo che punta già alla macchina. |
| `--login key` o `--login password` | Come accedi alla macchina d'ora in poi: con la chiave che usi già, o con una password. |
| `--user <name>` | L'account di tutti i giorni sulla macchina. Predefinito: quello con cui hai fatto l'accesso, o `otw` se hai fatto l'accesso come `root`. |
| `--admin-user <name>` | Il tuo nome di accesso a OpenTraderWorld. Predefinito: `admin`. |
| `--ssh-key <key>` | Una chiave pubblica autorizzata ad accedere. Ripetila per più di una. Predefinito: le chiavi già autorizzate. |
| `--ssh-port <n>` | La porta su cui si raggiunge la macchina. Predefinito: quella che usa già, altrimenti `22`. |
| `--dir <path>` | Dove installa. Predefinito: `/home/<user>/otw`. |
| `--timezone <zone>` | Il fuso orario della macchina, ad esempio `Europe/Paris`. Predefinito: lasciato com'è. |
| `--swap <size>` | Dimensione del file di swap, ad esempio `2G`, oppure `none`. Predefinito: `2G` su una macchina con meno di 4 GB di memoria. |
| `--yes` | Non chiede nulla, prende ogni valore predefinito. |

## Ancora bloccato?

1. Accedi alla tua macchina.
2. Digita:

   ```bash
   otw report
   ```

3. Scrive un file e stampa dove si trova. Il file non contiene password.
4. Apri una issue su [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues), racconta cosa stavi facendo e allega quel file.
