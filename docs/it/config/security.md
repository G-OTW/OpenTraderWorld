# Sicurezza dell'account

OTW ha un solo account e nessuna email di reset della password. Ciò che lo protegge è la password (o un
account social collegato), un secondo fattore opzionale, e il fatto che nient'altro che tu può
raggiungere la macchina. Questa pagina
copre cosa puoi attivare e cosa fare quando qualcosa va storto.

Tutto qui vive in **Impostazioni → Sicurezza**, tranne la password stessa, che resta in
**Impostazioni → Account**.

## Autenticazione a due fattori {#totp}

Un codice di sei cifre da un'app sul telefono, richiesto dopo la password. È ciò che una
password rubata da sola non può superare, ed è la cosa singola più utile da attivare
prima di aprire l'app a internet.

::: tip Non ti viene inviato nulla
Questo è **TOTP** (RFC 6238), non un codice inviato per SMS o email su richiesta. Il tuo authenticator e
il server condividono un segreto una volta, alla configurazione, poi ciascuno calcola lo stesso codice dall'ora
corrente. Nessun SMS, nessuna email, nessun servizio di terze parti, e funziona con la macchina offline.
:::

### Attivarla

1. **Impostazioni → Sicurezza → Configura**. L'app mostra un codice QR, il link `otpauth://` dietro
   di esso, e il segreto stesso.
2. Aggiungilo a qualsiasi app authenticator: Google Authenticator, Aegis, Ente Auth, 1Password,
   Bitwarden, Proton Pass, quella che già usi. Scansiona il codice QR, incolla il link,
   o digita il segreto a mano.
3. Digita le sei cifre che mostra e conferma.

Nulla sull'accesso cambia finché quest'ultimo passo non riesce. Un codice che non puoi produrre
non diventa mai un requisito, quindi una configurazione lasciata a metà non può bloccarti fuori.

### Accedere dopo

Inserisci nome utente e password come prima; l'app poi chiede il codice. Ogni codice funziona
una volta e dura 30 secondi, con un po' di tolleranza da entrambi i lati per un telefono il cui orologio
si è sfasato.

### Disattivarla

**Impostazioni → Sicurezza → Disattiva**, che chiede di nuovo la tua password. Fallo prima di
formattare un telefono, e configurala di nuovo sul nuovo.

### Se perdi l'authenticator {#totp-lost}

Non c'è codice di backup né email di recupero, per lo stesso motivo per cui non c'è un modulo di reset della
password. Recupera dalla shell della macchina:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

Il secondo fattore e il suo segreto vengono rimossi e ogni sessione viene disconnessa. Accedi con
la password, poi configuralo di nuovo sul nuovo dispositivo.

::: warning Tieni una via di rientro
Conserva il segreto nel tuo password manager quando lo configuri, oppure tieni attivo il backup
dell'authenticator stesso. Altrimenti l'unica via di rientro è l'accesso alla shell della macchina.
:::

## Social login {#social}

Accesso con Google, Microsoft, GitHub o OpenID Connect, vincolato a un account collegato, con
codici di recupero e un interruttore opzionale per disattivare le password. Vedi
[Social login](/it/config/social-login).

## Sessioni attive {#sessions}

**Impostazioni → Sicurezza** elenca ogni browser attualmente connesso all'account, con
l'indirizzo da cui è arrivato e quando è stato usato l'ultima volta. Quello in cui stai leggendo è segnato.

- **Chiudi** termina una di esse.
- **Chiudi ogni altra sessione** termina tutte tranne la tua, che è ciò da cliccare se hai effettuato l'accesso
  da qualche parte dove non dovevi, o non sei sicuro.

Un accesso da un indirizzo che l'account non ha mai usato prima genera anche una notifica. Arriva
nella campanella, ed è inviata a qualsiasi canale concesso al produttore **Security (sign-ins)**
in **Impostazioni → Notifiche** (un permesso wildcard lo copre già). Scatta una volta
per indirizzo, non a ogni accesso.

Le sessioni durano una settimana. Nelle modalità esposte in rete (**LAN + HTTPS** e **Pubblico**) terminano anche
dopo un giorno di inattività, così un browser lasciato aperto su una macchina da cui ti sei allontanato
non resta connesso indefinitamente. Su localhost e LAN semplice si applica solo il limite di una settimana.

## La richiesta della password che ritorna {#step-up}

Una manciata di azioni chiede di nuovo la tua password anche se sei già connesso:

- generare un token di accesso per un agent IA (**Impostazioni → Agent IA**)
- cambiare la modalità di rete (**Impostazioni → Rete**)
- scrivere un valore nel **Vault**
- scaricare un backup parziale **con le credenziali incluse**
- disattivare il secondo fattore

Queste o creano una credenziale, o cambiano ciò che il mondo esterno può raggiungere, o ti consegnano ogni
segreto salvato in un unico file. Una conferma le copre tutte per cinque minuti, così una serie di
modifiche chiede una volta, e la conferma appartiene al browser che l'ha data. Se il secondo
fattore è attivo, la richiesta chiede anche un codice.

## Regole per la password {#password}

Impostata o cambiata in **Impostazioni → Account**, che chiede sempre prima la password attuale
e disconnette ogni altra sessione in caso di successo.

Una password deve avere **almeno 12 caratteri** e non deve essere una che compare già negli
elenchi pubblici di violazioni. Quel controllo ignora le decorazioni che le persone aggiungono per aggirare una regola, quindi
`P@ssw0rd!2024` viene rifiutata per lo stesso motivo di `password`. Anche le sequenze (`abcdefghijkl`) e
qualsiasi cosa contenga il nome del tuo account sono rifiutate.

La password più facile che passa è qualche parola senza relazione: `fennel-ladder-oxide-73` è
accettata, breve e digitabile. Non c'è alcuna regola su mescolare simboli e cifre, perché
conta la lunghezza.

Dimenticata? Vedi [Password dimenticata](/it/guide/troubleshooting#forgot-password).

## Timeout delle richieste {#timeout}

**Impostazioni → Sicurezza** imposta anche per quanto tempo una singola richiesta può girare prima che il server la
fermi. Il predefinito è 120 secondi.

Esiste perché una richiesta bloccata non possa tenere aperta una connessione per sempre. **Non** è un rate
limit: nulla conta quanto spesso chiami l'app, e un agent IA che pilota intensamente l'API non
ne è toccato. Nemmeno gli stream di prezzi live e la vista dei log ne sono toccati, perché il limite copre la
produzione di una risposta, non la vita di uno stream.

Alzalo se un backtest lungo o un grosso import viene interrotto con un messaggio di timeout.

## Primo avvio su un'istanza esposta in rete {#setup-token}

Quando il primissimo account viene creato su un'istanza già raggiungibile dalla rete
(**LAN + HTTPS** o **Pubblico**), la procedura guidata chiede un **token di configurazione**. Vedi
[Rete e accesso remoto](/it/config/network#setup-token).
