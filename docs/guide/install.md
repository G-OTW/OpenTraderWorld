# Installation

Self-host OpenTraderWorld on your own machine or server. Takes about 5 minutes.

::: info Containerized only (for now)
OpenTraderWorld runs as a Docker Compose stack, the only supported deployment. A native install is possible but not recommended: Docker keeps the install non-intrusive (everything lives in containers and volumes) and quick to rebuild. See [Get Docker](/guide/docker) for why, and for per-OS install steps.
:::

## Requirements

- **Docker** with Docker Compose. Don't have it? [Get Docker](/guide/docker) covers macOS, Windows and Linux in a few commands.
- Linux, macOS, or Windows.
- A free port (**5454** by default; ports **80** + **443** for the HTTPS modes). You can change it during setup. The localhost and plain-HTTP LAN modes also bind **8443** on `127.0.0.1`.

Check Docker is ready:

```bash
docker --version
docker compose version
```

## One-command install (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

The installer checks Docker is ready, downloads the deploy files (only the `deploy/` directory, no source code, no toolchain) into `./opentraderworld`, then hands off to the guided setup below, which **pulls the prebuilt images** from Docker Hub.

Options go after `bash -s --`:

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash -s -- --dir ~/otw
```

| Option | Default | Notes |
|---|---|---|
| `--dir <path>` | `./opentraderworld` | Install directory. Refuses a non-empty directory or an existing install. |
| `--ref <ref>` | `master` | Branch or tag to install. |
| `--build` | off | Clone the full source and build the images locally instead of pulling (needs `git` and the toolchain). |
| `--yes` | off | Passed to `setup.sh`: take every default when there is no terminal to ask through. |
| `--wipe-volumes` | off | Passed to `setup.sh`: remove the volumes of a previous install without asking. **This deletes that install's data.** |

`OTW_DIR` and `OTW_REF` set the same values as `--dir` and `--ref` (a flag wins over its variable), and `OTW_REPO` the GitHub repository (default `G-OTW/OpenTraderWorld`).

## From a git clone (alternative)

```bash
git clone https://github.com/G-OTW/OpenTraderWorld.git
cd OpenTraderWorld/deploy
./setup.sh
```

## The guided setup

Both paths above run `deploy/setup.sh`. It asks a few questions, generates strong secrets, writes the config, and starts everything.

It prompts for:

| Question | Default | Notes |
|---|---|---|
| **Network mode** | `1` (localhost) | `1` this machine only · `2` LAN over plain HTTP · `3` LAN over HTTPS with a real certificate · `4` public internet at your own domain. Changeable later, see [Network & remote access](/config/network). |
| **Admin username** | `admin` | The admin account is created for you; a strong password is generated and shown **once**. |
| **HTTP port** | `5454` | Modes 1–2 only; modes 3 and 4 use 80 + 443. |
| **DuckDNS domain / token / LAN IP** | none | Mode 3 only. |
| **Public domain** | none | Mode 4 only, and it must already resolve to this server. |
| **Log level** | `info` | `trace` / `debug` / `info` / `warn` / `error`. |

It also takes these options:

| Option | Notes |
|---|---|
| `--build` | Build the images from source instead of pulling them (see below). |
| `--yes` | Take every default when there is no terminal to ask through. |
| `--wipe-volumes` | Remove the volumes of a previous install without asking. **This deletes that install's data.** |
| `--non-interactive` | Same as `OTW_NONINTERACTIVE=1`: read every answer from the environment, never from a terminal. An unset variable takes the default. |

The non-interactive variables, one per question: `NET_CHOICE` (`1` to `4`), `OTW_HTTP_PORT`, `ADMIN_USER`, `OTW_ADMIN_PASSWORD` (generated if unset), `OTW_LOG`, `DUCK_DOMAIN`, `DUCK_TOKEN` and `LAN_IP` (mode 3), `OTW_DOMAIN` (mode 4), and `OVERWRITE`, `CLEAN_VOLS`, `WIPE_VOL`, `START_NOW` (`y` or `N`) for the confirmations.

Database and session **secrets are generated automatically**, so you never type them. They are written to `deploy/.env` (file permissions `600`, never committed to git).

At the end, let the script start the stack: it waits for the API, **creates your admin account**, and prints the generated password **once**, so copy it before closing the terminal.

By default the setup **pulls the prebuilt images** from Docker Hub: no Rust or Node toolchain, first start in a couple of minutes. Run `./setup.sh --build` to build the three services from source instead (development, local changes).

::: tip Headless servers
The admin is created by core itself on first boot (from `deploy/.env`), so you don't need a browser on the server. Note the printed password and sign in from any machine that can reach the app.
:::

::: warning Reinstalling over previous data
If Docker volumes from a previous install exist, the setup offers to wipe them. The default is **No** everywhere: wiping (and losing the previous database) only happens on an explicit `y`. Fresh secrets over an old database volume can't work, so declining aborts the setup rather than starting a broken stack.
:::

## Manual install (alternative)

If you prefer to configure by hand:

```bash
cd deploy
cp .env.example .env
```

Edit `.env` and set at least:

- `POSTGRES_PASSWORD`: a strong password
- `DATABASE_URL`: must contain the same password, e.g. `postgres://otw:YOUR_PASSWORD@postgres:5432/opentraderworld`
- `SESSION_SECRET`: a long random string
- `OTW_SECRET_KEY`: another long random string, the key that encrypts stored credentials. Keep it stable: changing it later makes them unreadable.

Compose also needs the two network files that `setup.sh` would have written. For the localhost mode on port 5454:

```bash
cat > network.env <<'EOF'
OTW_BIND=127.0.0.1
OTW_HTTPS_BIND=127.0.0.1
OTW_HTTP_PORT=5454
OTW_HTTPS_PORT=8443
OTW_DOMAIN=:80
EOF
printf 'OTW_ACME_GLOBAL=\n' > dns.env
```

Both are rewritten from **Settings → Network** later, see [Network & remote access](/config/network).

Then start the stack. By default this **pulls the prebuilt images** from Docker Hub (no Rust or Node toolchain required):

```bash
docker compose -f docker-compose.yml -f docker-compose.images.yml \
  --env-file .env --env-file network.env up -d
```

::: details Build from source instead
For development, or to run local changes, omit the images override and build the three services yourself (needs the toolchain; the Rust build is slow):

```bash
docker compose --env-file .env --env-file network.env up --build -d
```
:::

## Create the admin (manual installs only)

If you used `./setup.sh` and let it start the stack, **your admin already exists**, so skip ahead.

Otherwise, open the app in your browser (`http://localhost:5454` by default). On the first visit, OpenTraderWorld detects there is no admin yet and shows the **setup wizard**: choose a username and password, submit, and you land on the dashboard. The password needs at least 12 characters, and a repeated character, a counted sequence, one containing the username or one from a known breach list is refused. Passwords are stored hashed with argon2.

In the HTTPS modes (LAN + HTTPS, public) the wizard also asks for a **setup token**, so that nobody who merely finds the address can claim the account first. Core prints it in its log at startup: `docker compose logs core`.

::: details Create the admin from the CLI (headless, no browser)
Hit the first-run endpoint from inside the stack: it works whatever your bind interface, and refuses (HTTP 409) if an admin already exists. In the HTTPS modes, add the setup token from core's log as one more header, `--header=X-OTW-Setup-Token:<token>`, or it answers 403 `setup_token_required`:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T caddy \
  wget -qO- --header=Content-Type:application/json \
  --post-data='{"username":"admin","password":"CHOOSE-A-STRONG-ONE"}' \
  http://core:8080/api/setup
```
:::

## Verify it's running

- App: `http://localhost:5454` (or your chosen port/domain)
- Health check: `http://localhost:5454/api/health` → `{"status":"ok"}` (signed in, it also returns the version, the uptime and the state of each service)

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## Everyday operations

Run these from `deploy/`. Every command takes both env files, and an image install also takes the images override, so start from the line that matches your install:

```bash
# image install (the default)
docker compose -f docker-compose.yml -f docker-compose.images.yml --env-file .env --env-file network.env
# source build
docker compose --env-file .env --env-file network.env
```

| Action | Add |
|---|---|
| Start | `up -d` |
| Stop | `down` |
| View logs | `logs -f` |
| Rebuild after changing code (source build) | `up --build -d` |
| Stop **and wipe all data** | `down -v` |

To move to a newer version, follow [Updating](/guide/updating): the image tags are pinned in `docker-compose.images.yml`, so a plain `pull` fetches the version you already run.

Your data lives in Docker named volumes and **persists** across `up`/`down`. It is only deleted with `down -v`.

## Next steps

- [First steps](/guide/first-steps): sign in, set defaults, install modules.
- [Network & remote access](/config/network): reach the app from other devices, LAN HTTPS, public exposure.
- Something wrong? See [Troubleshooting](/guide/troubleshooting).
