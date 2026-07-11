# Get Docker

OpenTraderWorld ships as a set of Docker containers, and that is currently the **only supported way to run it**. A native install (running the Rust core, PostgreSQL and the frontend directly on the host) is possible if you know what you're doing, but it is **not recommended or documented** — Docker is prioritized on purpose:

- **Non-intrusive** — nothing is installed on your system except Docker itself. The app, database and proxy live in containers; your data lives in named volumes. Removing everything is `docker compose down -v` plus deleting the folder.
- **Identical everywhere** — the same stack runs unchanged on macOS, Linux and Windows.
- **Quick to update** — refresh the repo and pull the new images (see [Updating](/guide/updating)); a broken container is recreated in seconds without touching your data.

If you already have Docker, jump straight to [Installation](/guide/install).

## macOS

Install **Docker Desktop**:

- Download it from [docker.com](https://www.docker.com/products/docker-desktop/) (pick Apple Silicon or Intel), open the `.dmg` and drag Docker to Applications — or with Homebrew:

  ```bash
  brew install --cask docker
  ```

- Launch **Docker** once from Applications and let it finish starting (whale icon in the menu bar stops animating).

Docker Compose is included.

## Windows

Install **Docker Desktop** with the WSL 2 backend:

1. Requirements: Windows 10/11 64-bit with **WSL 2** — enable it from an administrator PowerShell if needed: `wsl --install`, then reboot.
2. Install Docker Desktop from [docker.com](https://www.docker.com/products/docker-desktop/) — or:

   ```powershell
   winget install Docker.DockerDesktop
   ```

3. Launch Docker Desktop and keep the default *Use WSL 2* setting.

Run the OpenTraderWorld commands from any terminal (PowerShell or a WSL shell). Docker Compose is included.

## Linux

On a desktop or a headless server, install **Docker Engine** (no Desktop needed). The convenience script works on all major distributions:

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER   # run docker without sudo
newgrp docker                    # or log out and back in
sudo systemctl enable --now docker
```

Prefer your distribution's packages? See the [official per-distro instructions](https://docs.docker.com/engine/install/). Recent Engine installs include the Compose plugin.

## Verify

```bash
docker --version
docker compose version
docker run --rm hello-world
```

All three succeed → you're ready.

## Deploy OpenTraderWorld

Three commands, then follow the prompts:

```bash
git clone https://github.com/G-OTW/OpenTraderWorld.git
cd OpenTraderWorld/deploy
./setup.sh
```

The full walkthrough (what the prompts mean, manual alternative, verifying the result) is on the [Installation](/guide/install) page.

::: info Images are built locally for now
The first start **builds the images from source** on your machine, which takes a few minutes depending on hardware. Prebuilt images on Docker Hub are planned — deploys will then skip the build entirely.
:::
