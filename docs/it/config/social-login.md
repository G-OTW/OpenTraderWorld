# Social login

Accedi con un account Google, Microsoft, GitHub o OpenID Connect (Authentik, Keycloak,
Authelia, Zitadel…).

L'accesso è vincolato all'account che colleghi, tramite l'id utente del provider, non l'e-mail. Un altro
account presso lo stesso provider viene rifiutato. Se l'autenticazione a due fattori è attiva, il codice
viene comunque richiesto.

Tutte le impostazioni sono in **Impostazioni → Sicurezza → Social login**. Ogni modifica chiede la tua
password.

## Requisiti {#requirements}

- **URI di reindirizzamento**: `<indirizzo di OTW>/auth/social`, ad es. `https://otw.example.com/auth/social`.
  Le Impostazioni mostrano il valore esatto per l'indirizzo in cui ti trovi (clic per copiare). Deve corrispondere
  esattamente all'indirizzo del browser: schema, host e porta.
- **Google**: `https://` e un nome di dominio. `http://` semplice e indirizzi IP nudi sono rifiutati,
  tranne `localhost` / `127.0.0.1`.
- **Microsoft**: `https://`, oppure `http://localhost`.
- **GitHub** e provider self-hosted: ciò che permettono.

Un'istanza raggiunta in HTTP semplice su un indirizzo LAN deve prima passare a `lan_https` o `web`
per Google e Microsoft ([Rete](/it/config/network)).

## Configurazione {#setup}

### 1. Registra OTW presso il provider

::: details Google
1. [Google Cloud console](https://console.cloud.google.com/) → **Google Auth Platform**.
   Al primo utilizzo, compila il nome dell'app e l'e-mail di supporto, pubblico **Esterno**.
2. **Pubblico**: finché l'app è in *Testing*, possono accedere solo gli utenti di test elencati. Aggiungi lì il tuo
   account Google.
3. **Clients → Create client**, tipo **Web application**.
4. **Authorized redirect URIs**: l'URI di reindirizzamento dalle Impostazioni. Lascia vuoti gli *Authorized JavaScript
   origins*.
5. Copia l'ID client e il secret client. Google può impiegare qualche minuto ad applicare un nuovo
   URI di reindirizzamento.
:::

::: details Microsoft
1. [Microsoft Entra admin center](https://entra.microsoft.com/) → **App registrations → New
   registration**.
2. **Supported account types**: includi gli account personali se accedi con uno
   (Outlook.com, Hotmail, Xbox).
3. **Authentication → Add a platform → Web**: l'URI di reindirizzamento dalle Impostazioni.
4. **Certificates & secrets → Client secrets → New client secret**. Copia il **Value**, viene
   mostrato una volta sola. Scade (24 mesi al massimo), vedi [Secret scaduto](#secret-expired).
5. Copia l'**Application (client) ID** da **Overview**.
6. Tenant in OTW: `common` (predefinito, qualsiasi account), `consumers` (solo personali),
   `organizations` (solo lavoro o scuola), oppure l'ID o il dominio del tuo tenant.
:::

::: details GitHub
1. GitHub → **Settings → Developer settings → OAuth Apps → New OAuth App**.
2. **Authorization callback URL**: l'URI di reindirizzamento dalle Impostazioni.
3. Copia l'ID client, poi **Generate a new client secret** e copialo.
:::

::: details OpenID Connect (self-hosted)
1. Crea un client OpenID Connect confidenziale: flusso authorization code, scope
   `openid email profile`, l'URI di reindirizzamento dalle Impostazioni.
2. Copia l'ID client, il secret client e l'**URL dell'issuer**: l'indirizzo che serve
   `/.well-known/openid-configuration`, ad es. `https://auth.example.com/application/o/otw` su
   Authentik.
3. L'issuer deve essere `https://` (`http://` solo su localhost) e deve corrispondere esattamente al campo `issuer`
   di quel documento.
:::

### 2. Inseriscilo in OTW

Scegli il provider, incolla ID client e secret (e il tenant o l'URL dell'issuer), **Salva**.
OTW contatta il provider al salvataggio: un tenant o un issuer sbagliato fallisce qui, non all'accesso.

### 3. Collega il tuo account

**Collega un account** ti manda al provider per scegliere l'account. Tornato nelle Impostazioni, la
scheda mostra *Vincolato a …*. La pagina di login ora ha un pulsante **Continua con …**.

### 4. Genera i codici di recupero {#recovery-codes}

**Codici di recupero → Genera**: dieci codici monouso, mostrati una sola volta. Ti fanno accedere quando
l'account del provider è bloccato, eliminato o irraggiungibile. Conservali fuori da questo server. Una nuova serie
annulla la precedente.

### 5. Disattiva l'sign-in con password (facoltativo) {#password-off}

Possibile una volta collegato un account e creati i codici di recupero. **Password sign-in → Disattiva**:
il modulo di login rifiuta le password, funzionano solo l'account collegato e i codici di recupero.

Si riattiva da solo con scollegamento, rimozione, accesso con codice di recupero e reset
della password dall'host.

## Ripristino {#rollback}

### Ancora connesso

- **Password sign-in → Attiva**: le password funzionano di nuovo, l'social login resta.
- **Scollega**: l'social login si ferma, le password funzionano di nuovo. Le impostazioni del provider restano.
- **Rimuovi**: impostazioni del provider e collegamento eliminati, le password funzionano di nuovo.

### Account del provider inutilizzabile

Pagina di login → **Usa un codice di recupero**. Accedi, l'sign-in con password è di nuovo attivo, e
si apre Impostazioni → Sicurezza. Scollegare o collegare un altro account lì non chiede la
password per i successivi cinque minuti.

### Persi anche i codici di recupero

Sull'host:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Rimuove il collegamento, riattiva l'sign-in con password e disconnette ogni sessione. Se anche la password è
persa, esegui poi `reset-password` ([Password
dimenticata](/it/guide/troubleshooting#forgot-password)). `list-users` stampa il nome utente.

### Tornare alla versione precedente {#downgrade}

Questa release aggiunge la migrazione `0145_social_login`. Una versione precedente rifiuta di avviarsi su un
database con una migrazione che non conosce, quindi rimuovila prima:

1. Fai il backup del database ([Backup e ripristino](/it/guide/backup-restore)).
2. Da `deploy/`:

   ```bash
   docker compose --env-file .env --env-file network.env exec -T postgres \
     psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -c "
       BEGIN;
       DROP TABLE IF EXISTS recovery_codes;
       DROP TABLE IF EXISTS social_login;
       ALTER TABLE users DROP COLUMN IF EXISTS password_login;
       DELETE FROM _sqlx_migrations WHERE version = 145;
       COMMIT;"
   ```

3. Installa la release precedente ([Aggiornamento](/it/guide/updating)): build da sorgenti,
   `git reset --hard <previous release commit>` poi `up -d --build`; installazione da immagini, il
   `deploy/` e le immagini della release precedente.

Le password funzionano sulla versione precedente qualunque fosse l'impostazione dell'interruttore. Le impostazioni del provider e
i codici di recupero vengono eliminati.

::: warning
Questo rimuove solo la migrazione 145. Se è installata una release successiva con più migrazioni,
ripristina invece il backup fatto prima di quell'aggiornamento.
:::

Il passo 2 senza il passo 3 azzera l'social login: la versione attuale ricrea le tabelle vuote
al prossimo avvio.

## Errori {#errors}

| Messaggio | Soluzione |
|---|---|
| `redirect_uri_mismatch` (Google), `AADSTS50011` (Microsoft) | L'URI di reindirizzamento registrato differisce dall'indirizzo del browser. Copialo di nuovo dalle Impostazioni. |
| *The OAuth client was not found* / `invalid_client` | ID client o secret sbagliati, oppure il secret è scaduto. |
| *the provider calls itself …* | L'URL dell'issuer differisce dall'`issuer` in `/.well-known/openid-configuration` del provider. |
| *this … account is not the one linked to this instance* | Al provider è stato scelto un altro account. |
| *this sign-in was started in another browser or has expired* | Sono passati più di dieci minuti, oppure l'accesso è terminato in un altro browser. Ricomincia. |
| *the ID token has expired* | L'orologio del server è sbagliato. Correggi l'ora dell'host (NTP). |

### Secret scaduto {#secret-expired}

L'accesso fallisce con `invalid_client` o con un messaggio su un secret scaduto. Crea un nuovo secret
presso il provider, poi **Modifica** nelle Impostazioni, incollalo, **Salva**. Il collegamento resta. Bloccato fuori?
Accedi prima con un codice di recupero.
