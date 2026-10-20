# Rete e accesso remoto

OpenTraderWorld controlla **chi può raggiungere l'app** tramite quattro modalità di rete. Dopo l'installazione è **solo localhost**: nulla sulla tua rete può connettersi finché non cambi questa impostazione.

Il modo supportato per cambiare modalità è nell'app: **Impostazioni → Rete**. Salvando viene mostrato il comando di riavvio esatto da eseguire sull'host (l'app non può riavviare i propri container); lo stack è brevemente offline mentre i container vengono ricreati.

## Le quattro modalità

| Modalità | Raggiungibile da | Protocollo | Usala quando |
|---|---|---|---|
| **Solo localhost** | questa macchina | HTTP | Predefinita. La più sicura, nulla sulla rete può connettersi. |
| **Rete locale (LAN)** | dispositivi sulla tua rete, tramite l'IP della macchina | HTTP semplice | Accesso rapido in LAN; va bene per reti domestiche fidate. I browser possono avvisare o forzare l'upgrade a HTTPS, e rifiutano il microfono, quindi il [controllo vocale](/it/config/voice#https) non funziona da altri dispositivi. |
| **LAN + HTTPS** | dispositivi sulla tua rete, tramite un dominio reale | HTTPS, certificato attendibile | Accesso in LAN senza avvisi del browser. Nulla esposto a internet. |
| **Pubblico (Web)** | chiunque, sul tuo dominio | HTTPS (Let's Encrypt) | Vuoi accedere da ovunque e accetti l'esposizione pubblica. |

## LAN + HTTPS (senza avvisi del browser) {#lan-https}

I browser rifiutano o avvisano sempre più spesso sui siti in HTTP semplice. Questa modalità serve l'app a ogni dispositivo della tua rete con un **certificato reale, attendibile pubblicamente**: nessun avviso, nulla da installare sui dispositivi client e **nulla esposto a internet**. La proprietà del dominio viene dimostrata con un record DNS (challenge ACME DNS-01), non con una connessione in ingresso, e il dominio risolve verso l'IP LAN privato della tua macchina.

Configurala durante l'installazione (`./setup.sh`, modalità `3`) o in seguito in **Impostazioni → Rete → Rete locale (LAN) + HTTPS**:

1. Accedi a [duckdns.org](https://www.duckdns.org) (gratuito), aggiungi un sottodominio (ad es. `myotw.duckdns.org`) e copia il token del tuo account, oppure usa un tuo dominio su Cloudflare con un token API di modifica DNS.
2. Inserisci dominio + token, più l'IP LAN della tua macchina (con DuckDNS il record viene puntato automaticamente; su Cloudflare crea tu il record A).
3. Applica con il comando di riavvio mostrato. La prima richiesta può richiedere ~30 s mentre il certificato viene emesso.

Poi apri `https://myotw.duckdns.org` da qualsiasi dispositivo della tua rete.

::: warning Note
- I certificati emessi compaiono nei log pubblici di Certificate Transparency, quindi il **nome** del dominio è pubblicamente visibile (l'app stessa resta solo LAN).
- Il token DNS è salvato in `deploy/dns.env`: non committare né condividere mai quel file.
- Questa modalità usa le porte **80 + 443** al posto della porta personalizzata.
:::

### Se il dominio non si risolve su alcuni dispositivi {#dns-rebind}

Alcuni router/resolver degli ISP scartano in silenzio le risposte DNS che puntano a un IP privato ("DNS rebind protection"). Soluzioni, dalla migliore:

1. **Consenti il dominio** nelle impostazioni del router/DNS.
2. **Abilita il DNS sicuro (DNS over HTTPS)** nel browser. Chrome: Impostazioni → Privacy e sicurezza → Sicurezza → *Usa DNS sicuro* → Cloudflare; Firefox: Impostazioni → Privacy → *DNS over HTTPS* → Massima protezione.
3. **Soluzione con file hosts** (per singola macchina, e i telefoni non possono farlo). Associa il dominio all'IP LAN del server:

   ```bash
   # macOS / Linux, then flush the cache (macOS only):
   echo "192.168.1.50 myotw.duckdns.org" | sudo tee -a /etc/hosts
   sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder
   ```

   Su Windows, modifica `C:\Windows\System32\drivers\etc\hosts` come Amministratore, aggiungi la stessa riga, poi esegui `ipconfig /flushdns`. Il file hosts vince sempre sul DNS, quindi rimuovi la riga se l'IP del server cambia.

## Pubblico (Web) {#public}

Espone l'app sul tuo dominio con HTTPS automatico.

**Prerequisiti:**

1. Un **dominio** con un **record A / AAAA** pubblico che punta all'IP pubblico del tuo server.
2. TCP **80** e **443** in ingresso che raggiungano il server: aprili nel firewall del router/cloud e fai port-forward se sei dietro NAT. La porta 80 serve per il certificato (challenge HTTP-01) e reindirizza a HTTPS.

Scegli la modalità `4` in `./setup.sh` o cambia in **Impostazioni → Rete**. Caddy ottiene e rinnova automaticamente un certificato Let's Encrypt alla prima richiesta (~30 s).

::: danger Chiunque può raggiungere la pagina di login una volta attivata
Abilitala solo dopo che il tuo account admin esiste e con una password robusta. Attiva
l'[autenticazione a due fattori](/it/config/security#totp) prima di farlo. Tieni segreto `deploy/.env`.
Torna a una modalità privata in qualsiasi momento in Impostazioni → Rete.
:::

## Il token di configurazione {#setup-token}

Su **LAN + HTTPS** e **Pubblico**, la procedura guidata del primo avvio chiede un **token di configurazione** prima di
creare il primo account.

Il motivo è una corsa: queste modalità rispondono alla rete, e la procedura guidata resta aperta finché non
esiste un account. Senza token, chi carica la pagina per primo diventa l'amministratore, il che è un
rischio reale nell'intervallo in cui il DNS si propaga, e di nuovo se un volume del database viene mai ricreato.

Il core genera il token all'avvio e lo stampa nel suo log:

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env logs core | grep "setup token"
```

Incollalo nel campo extra che la procedura mostra. Ad ogni riavvio ne viene generato uno nuovo, quindi un
token abbandonato smette di funzionare appena il container si riavvia.

Non lo vedrai se hai installato con `./setup.sh`: crea l'account da
`deploy/.env` al primo avvio, prima che qualsiasi cosa possa raggiungere la procedura guidata. Il token compare solo
quando non esiste ancora alcun account.

## Cambiare la modalità dalla CLI {#change-mode-cli}

Se hai scelto la modalità sbagliata all'installazione e non riesci a raggiungere affatto l'app (ad es. hai scelto *localhost* su un server headless), modifica direttamente `deploy/network.env` e riavvia:

```bash
cd deploy
# make it reachable on your LAN over plain HTTP (mode 2):
#   OTW_BIND=0.0.0.0     (was 127.0.0.1)
#   OTW_HTTP_PORT=5454   (or your chosen port)
$EDITOR network.env
docker compose --env-file .env --env-file network.env up -d
```

`network.env` è privo di segreti: contiene l'interfaccia di bind (`127.0.0.1` = solo questa macchina, `0.0.0.0` = tutte le interfacce) e le porte, interpolate da Compose. LAN + HTTPS richiede più di una riga (certificato + token DNS), quindi configurala da Impostazioni o da `./setup.sh` modalità `3`. Una volta che riesci ad aprire l'app, usa **Impostazioni → Rete**.
