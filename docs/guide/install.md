# Installation

Self-host OpenTraderWorld on your own machine or server. Takes about 5 minutes.

::: info Containerized only (for now)
OpenTraderWorld runs as a Docker Compose stack — the only supported deployment. A native install is possible but not recommended: Docker keeps the install non-intrusive (everything lives in containers and volumes) and quick to rebuild. See [Get Docker](/guide/docker) for why, and for per-OS install steps.
:::

## Requirements

- **Docker** with Docker Compose — don't have it? [Get Docker](/guide/docker) covers macOS, Windows and Linux in a few commands.
- Linux, macOS, or Windows.
- A free port (**5454** by default; ports **80** + **443** for the HTTPS modes). You can change it during setup.

Check Docker is ready:

```bash
docker --version
docker compose version
```

## One-command install (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

The installer checks Docker is ready, downloads the deploy files (only the `deploy/` directory — no source code, no toolchain) into `./opentraderworld`, then hands off to the guided setup below, which **pulls the prebuilt images** from Docker Hub.

Options go after `bash -s --`:

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash -s -- --dir ~/otw
```

| Option | Default | Notes |
|---|---|---|
| `--dir <path>` | `./opentraderworld` | Install directory. Refuses a non-empty directory or an existing install. |
| `--ref <ref>` | `master` | Branch or tag to install. |
| `--build` | off | Clone the full source and build the images locally instead of pulling (needs `git` and the toolchain). |

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
| **Network mode** | `1` (localhost) | `1` this machine only · `2` LAN over plain HTTP · `3` LAN over HTTPS with a real certificate · `4` public internet at your own domain. Changeable later — see [Network & remote access](/config/network). |
| **Admin username** | `admin` | The admin account is created for you; a strong password is generated and shown **once**. |
| **HTTP port** | `5454` | Modes 1–2 only; mode 3 uses 80 + 443. |
| **DuckDNS domain / token / LAN IP** | — | Mode 3 only. |
| **Log level** | `info` | `trace` / `debug` / `info` / `warn` / `error`. |

Database and session **secrets are generated automatically** — you never type them. They are written to `deploy/.env` (file permissions `600`, never committed to git).

At the end, let the script start the stack: it waits for the API, **creates your admin account**, and prints the generated password **once** — copy it before closing the terminal.

By default the setup **pulls the prebuilt images** from Docker Hub — no Rust or Node toolchain, first start in a couple of minutes. Run `./setup.sh --build` to build the three services from source instead (development, local changes).

::: tip Headless servers
The admin is created by core itself on first boot (from `deploy/.env`), so you don't need a browser on the server. Note the printed password and sign in from any machine that can reach the app.
:::

::: warning Reinstalling over previous data
If Docker volumes from a previous install exist, the setup offers to wipe them. The default is **No** everywhere — wiping (and losing the previous database) only happens on an explicit `y`. Fresh secrets over an old database volume can't work, so declining aborts the setup rather than starting a broken stack.
:::

## Manual install (alternative)

If you prefer to configure by hand:

```bash
cd deploy
cp .env.example .env
```

Edit `.env` and set at least:

- `POSTGRES_PASSWORD` — a strong password
- `DATABASE_URL` — must contain the same password, e.g. `postgres://otw:YOUR_PASSWORD@postgres:5432/opentraderworld`
- `SESSION_SECRET` — a long random string

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

If you used `./setup.sh` and let it start the stack, **your admin already exists** — skip ahead.

Otherwise, open the app in your browser (`http://localhost:5454` by default). On the first visit, OpenTraderWorld detects there is no admin yet and shows the **setup wizard**: choose a username and password (min 8 characters), submit, and you land on the dashboard. Passwords are stored hashed with argon2.

::: details Create the admin from the CLI (headless, no browser)
Hit the first-run endpoint from inside the stack — it works regardless of your bind interface or TLS mode, and refuses (HTTP 409) if an admin already exists:

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
- Health check: `http://localhost:5454/api/health` → `{"status":"ok","service":"otw-core",...}`

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## Everyday operations

Run these from `deploy/`:

| Action | Command |
|---|---|
| Start | `docker compose up -d` |
| Stop | `docker compose down` |
| View logs | `docker compose logs -f` |
| Pull newer images | `docker compose -f docker-compose.yml -f docker-compose.images.yml pull && docker compose up -d` |
| Rebuild after changing code (source build) | `docker compose up --build -d` |
| Stop **and wipe all data** | `docker compose down -v` |

Your data lives in Docker named volumes and **persists** across `up`/`down`. It is only deleted with `down -v`.

## Next steps

- [First steps](/guide/first-steps) — sign in, set defaults, install modules.
- [Network & remote access](/config/network) — reach the app from other devices, LAN HTTPS, public exposure.
- Something wrong? See [Troubleshooting](/guide/troubleshooting).
