# Docker besorgen

OpenTraderWorld wird als Satz von Docker-Containern ausgeliefert, und das ist derzeit der **einzige unterstützte Weg, es zu betreiben**. Eine native Installation (Rust-Core, PostgreSQL und Frontend direkt auf dem Host) ist möglich, wenn du weißt, was du tust, wird aber **weder empfohlen noch dokumentiert**. Docker hat bewusst Vorrang:

- **Unaufdringlich**: Außer Docker selbst wird nichts auf deinem System installiert. App, Datenbank und Proxy laufen in Containern; deine Daten liegen in benannten Volumes. Alles zu entfernen heißt `docker compose down -v` und das Löschen des Ordners.
- **Überall identisch**: Derselbe Stack läuft unverändert auf macOS, Linux und Windows.
- **Schnell aktualisiert**: Repo aktualisieren und die neuen Images ziehen (siehe [Aktualisieren](/de/guide/updating)); ein defekter Container ist in Sekunden neu erstellt, ohne deine Daten anzutasten.

Wenn du Docker schon hast, springe direkt zur [Installation](/de/guide/install).

## macOS

Installiere **Docker Desktop**:

- Lade es von [docker.com](https://www.docker.com/products/docker-desktop/) herunter (Apple Silicon oder Intel wählen), öffne die `.dmg` und ziehe Docker nach Programme, oder mit Homebrew:

  ```bash
  brew install --cask docker
  ```

- Starte **Docker** einmal aus Programme und warte, bis der Start abgeschlossen ist (das Wal-Symbol in der Menüleiste hört auf zu animieren).

Docker Compose ist enthalten.

## Windows

Installiere **Docker Desktop** mit dem WSL-2-Backend:

1. Voraussetzungen: Windows 10/11 64-Bit mit **WSL 2**, bei Bedarf aus einer PowerShell mit Administratorrechten aktiviert: `wsl --install`, danach neu starten.
2. Installiere Docker Desktop von [docker.com](https://www.docker.com/products/docker-desktop/), oder:

   ```powershell
   winget install Docker.DockerDesktop
   ```

3. Starte Docker Desktop und lass die Standardeinstellung *Use WSL 2* aktiv.

Führe die OpenTraderWorld-Befehle in einem beliebigen Terminal aus (PowerShell oder eine WSL-Shell). Docker Compose ist enthalten.

## Linux

Installiere auf einem Desktop oder einem Server ohne Oberfläche die **Docker Engine** (Desktop ist nicht nötig). Das Komfortskript funktioniert auf allen gängigen Distributionen:

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER   # run docker without sudo
newgrp docker                    # or log out and back in
sudo systemctl enable --now docker
```

Lieber die Pakete deiner Distribution? Siehe die [offizielle Anleitung je Distribution](https://docs.docker.com/engine/install/). Neuere Engine-Installationen enthalten das Compose-Plugin.

## Prüfen

```bash
docker --version
docker compose version
docker run --rm hello-world
```

Alle drei gelingen → du bist bereit.

## OpenTraderWorld bereitstellen

Ein Befehl, dann den Eingaben folgen:

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

Die vollständige Anleitung (was die Eingaben bedeuten, Optionen, manuelle Alternative, Ergebnis prüfen) steht auf der Seite [Installation](/de/guide/install).

::: info Vorgefertigte Images
Die Installation **zieht vorgefertigte Images** von Docker Hub: kein Build, keine Rust-/Node-Toolchain. Das Bauen aus dem Quellcode bleibt für die Entwicklung möglich (`install.sh --build`, oder `./setup.sh --build` aus einem Klon).
:::
