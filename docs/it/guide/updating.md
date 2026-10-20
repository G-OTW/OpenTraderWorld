# Aggiornamento

OpenTraderWorld ti avvisa quando è disponibile una nuova versione in **Impostazioni → Aggiorna app** (controlla GitHub). L'app **non può aggiornarsi da sola per scelta progettuale** (gira senza accesso a shell o Docker, per ridurre la superficie d'attacco), quindi gli aggiornamenti sono un paio di comandi sull'host.

## Prima di aggiornare

1. **Fai un backup del database**: vedi [Backup e ripristino](/it/guide/backup-restore).
2. Scorri le note di rilascio per eventuali modifiche incompatibili.

## Che installazione hai?

Guarda la tua cartella di installazione:

- Solo `deploy/` all'interno, nessun `.git`: **installazione da immagini** (l'installer a un comando, o
  `setup.sh` senza `--build`). È il caso predefinito.
- `core/`, `frontend/` e un `.git`: **build da sorgenti** (`install.sh --build`, oppure un clone
  più `./setup.sh --build`).

## Installazione da immagini

Non viene compilato nulla e non c'è un checkout git, quindi aggiorna `deploy/` dalla release
(è lì che sono fissati i nuovi tag delle immagini), poi scarica e riavvia:

```bash
cd /path/to/opentraderworld
TMP=$(mktemp -d)
curl -fsSL https://codeload.github.com/G-OTW/OpenTraderWorld/tar.gz/master | tar -xz -C "$TMP"
cp -R "$TMP"/*/deploy/. deploy/
rm -rf "$TMP"
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  pull
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d
```

La tua configurazione sopravvive: `.env`, `network.env` e `dns.env` non fanno parte della
release, quindi la copia non li sovrascrive mai. Tutto il resto in `deploy/` viene sostituito dalla
nuova versione, ed è lo scopo: le modifiche locali a `docker-compose.yml` o `Caddyfile`
si perdono, conservale in una patch se ti servono.

## Build da sorgenti

Aggiorna il checkout, poi ricompila:

```bash
cd /path/to/OpenTraderWorld
git fetch origin
git reset --hard origin/master
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d --build
```

::: warning Usa `git reset --hard`, non `git pull`
Ogni release viene pubblicata come uno snapshot nuovo del repository, quindi `git pull` segnala
"divergent branches" e fallisce. `git reset --hard origin/master` fa combaciare il tuo checkout
esattamente con la nuova release. I tuoi dati e la configurazione restano intatti: vivono nei
volumi Docker e in `.env` / `network.env`, che non sono tracciati da git. Se hai modificato
file tracciati in locale, mettili prima da parte (`git stash`).
:::

Per scaricare le immagini pubblicate invece di ricompilare dal tuo checkout, aggiungi
`-f deploy/docker-compose.images.yml` e usa `pull` + `up -d` come nell'installazione da immagini.

Ecco fatto:

- I container vengono ricreati con la nuova versione e riavviati.
- Le **migrazioni del database vengono eseguite automaticamente** al primo avvio del nuovo container core.
- I tuoi dati restano intatti: vivono nei volumi Docker, indipendenti dalle immagini.

L'app è brevemente offline mentre i container vengono ricreati. I comandi esatti (con i tuoi percorsi configurati) sono mostrati anche in **Impostazioni → Aggiorna app**.

## Una tantum: OAuth per gli agent IA (0.0.16) {#oauth-caddyfile}

Il sign-in OAuth per i client MCP richiede che i documenti di discovery sotto `/.well-known/oauth-*`
raggiungano il core. La procedura per l'installazione da immagini qui sopra e le build da sorgenti portano con sé il nuovo
`deploy/Caddyfile`, come le nuove installazioni. Un'installazione aggiornata con `otw update` mantiene
il suo vecchio Caddyfile: tutto funziona tranne OAuth finché non aggiungi questo blocco, subito prima della
riga `# Everything else → static frontend`:

```
	handle /.well-known/oauth-* {
		header Content-Security-Policy "default-src 'none'; frame-ancestors 'none'"
		reverse_proxy core:8080
	}
```

Poi ricrea Caddy (un reload non basta: la maggior parte degli editor sostituisce il file, e il
container tiene quello vecchio):

```bash
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d --force-recreate caddy
```

## Una tantum: ritirare la password di bootstrap {#retire-bootstrap-password}

Se il core scrive questo all'avvio, ce l'ha con te:

```
OTW_ADMIN_PASSWORD is still set but the admin account already exists.
```

`setup.sh` crea l'admin da `deploy/.env` al primo avvio e poi svuota quella riga. In
un'installazione fatta prima che lo facesse, il valore è ancora nel file, e nell'ambiente
del container dove `docker inspect` lo consegna a chiunque possa raggiungere il demone
Docker. A questo punto non concede nulla, è semplicemente un'altra copia di una password su disco.

```bash
sed -i.bak 's/^OTW_ADMIN_PASSWORD=.*/OTW_ADMIN_PASSWORD=/' deploy/.env
chmod 600 deploy/.env && rm -f deploy/.env.bak
```

Poi ricrea il core così sparisce anche dall'ambiente:

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d core
```

Tieni `OTW_ADMIN_USER`: una password vuota rende l'intero bootstrap un'operazione nulla, che è ciò che
vuoi, e la riga documenta comunque come un'installazione headless crea il suo primo account.
