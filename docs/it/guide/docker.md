# Installare Docker

OpenTraderWorld è distribuito come un insieme di container Docker, e attualmente è l'**unico modo supportato per eseguirlo**. Un'installazione nativa (eseguire direttamente sull'host il core Rust, PostgreSQL e il frontend) è possibile se sai cosa fai, ma **non è consigliata né documentata**. Docker è privilegiato di proposito:

- **Non invasivo**: sul sistema non viene installato nulla tranne Docker stesso. App, database e proxy vivono nei container; i tuoi dati vivono in volumi con nome. Rimuovere tutto significa `docker compose down -v` più eliminare la cartella.
- **Identico ovunque**: lo stesso stack gira senza modifiche su macOS, Linux e Windows.
- **Veloce da aggiornare**: aggiorna il repo e scarica le nuove immagini (vedi [Aggiornamento](/it/guide/updating)); un container danneggiato viene ricreato in pochi secondi senza toccare i tuoi dati.

Se hai già Docker, passa direttamente a [Installazione](/it/guide/install).

## macOS

Installa **Docker Desktop**:

- Scaricalo da [docker.com](https://www.docker.com/products/docker-desktop/) (scegli Apple Silicon o Intel), apri il `.dmg` e trascina Docker in Applicazioni, oppure con Homebrew:

  ```bash
  brew install --cask docker
  ```

- Avvia **Docker** una volta da Applicazioni e lascialo finire di avviarsi (l'icona della balena nella barra dei menu smette di animarsi).

Docker Compose è incluso.

## Windows

Installa **Docker Desktop** con il backend WSL 2:

1. Requisiti: Windows 10/11 a 64 bit con **WSL 2**, abilitato se necessario da una PowerShell da amministratore: `wsl --install`, poi riavvia.
2. Installa Docker Desktop da [docker.com](https://www.docker.com/products/docker-desktop/), oppure:

   ```powershell
   winget install Docker.DockerDesktop
   ```

3. Avvia Docker Desktop e lascia l'impostazione predefinita *Use WSL 2*.

Esegui i comandi di OpenTraderWorld da qualsiasi terminale (PowerShell o una shell WSL). Docker Compose è incluso.

## Linux

Su un desktop o un server headless, installa **Docker Engine** (Desktop non serve). Lo script di comodo funziona su tutte le principali distribuzioni:

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER   # run docker without sudo
newgrp docker                    # or log out and back in
sudo systemctl enable --now docker
```

Preferisci i pacchetti della tua distribuzione? Vedi le [istruzioni ufficiali per distribuzione](https://docs.docker.com/engine/install/). Le installazioni recenti di Engine includono il plugin Compose.

## Verifica

```bash
docker --version
docker compose version
docker run --rm hello-world
```

Tutti e tre riescono → sei pronto.

## Distribuire OpenTraderWorld

Un solo comando, poi segui le richieste:

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

La procedura completa (cosa significano le richieste, opzioni, alternativa manuale, verifica del risultato) è nella pagina [Installazione](/it/guide/install).

::: info Immagini precompilate
L'installazione **scarica immagini precompilate** da Docker Hub: nessuna build, nessuna toolchain Rust/Node. La compilazione dai sorgenti resta disponibile per lo sviluppo (`install.sh --build`, oppure `./setup.sh --build` da un clone).
:::
