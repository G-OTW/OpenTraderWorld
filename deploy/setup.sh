#!/usr/bin/env bash
#
# OpenTraderWorld — infra setup.
#
# Interactively collects host-level configuration (ports, domain) and generates
# strong secrets, writes deploy/.env, then optionally brings the stack up.
#
# The ADMIN ACCOUNT is NOT created here — it is created in the browser on first
# visit (the in-app first-run wizard), because it needs the database running.
#
# Usage:  ./setup.sh            # pull prebuilt images from Docker Hub (fast, default)
#         ./setup.sh --build    # build images from source instead (for development)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/.env"

# Install mode: pull published images (default) or build from source (--build).
BUILD_FROM_SOURCE=0
# Accept every prompt's default without a terminal (CI, `curl | bash` with no tty).
ASSUME_DEFAULTS="${OTW_ASSUME_YES:-0}"
# Remove leftover data volumes without asking. Destroys the previous database.
WIPE_VOLUMES=0
for arg in "$@"; do
  case "$arg" in
    --build) BUILD_FROM_SOURCE=1 ;;
    -y|--yes) ASSUME_DEFAULTS=1 ;;
    --wipe-volumes) WIPE_VOLUMES=1 ;;
    -h|--help) grep -E '^# (Usage|        )' "$0" | sed 's/^# //'; exit 0 ;;
    *) echo "Unknown option: $arg (see --help)" >&2; exit 1 ;;
  esac
done

bold() { printf '\033[1m%s\033[0m\n' "$1"; }
die()  { printf 'error: %s\n' "$1" >&2; exit 1; }
info() { printf '  %s\n' "$1"; }

# Prompt with a default: prompt_default VAR "Question" "default"
# On EOF (no terminal, e.g. `curl | bash` without a tty) `read` returns non-zero with an
# empty answer, which is indistinguishable from the user pressing enter. Silently taking
# defaults there invents an admin account and a network mode nobody chose, so stop instead
# and point at the non-interactive escape hatch.
prompt_default() {
  local __var="$1" __q="$2" __def="$3" __ans
  # Read straight from the controlling terminal when stdin isn't one. Under
  # `curl | bash` stdin is the pipe, but a real terminal is still reachable at
  # /dev/tty — per-read redirection is far more robust than reattaching stdin
  # process-wide, which breaks across the exec into this script.
  if [[ ! -t 0 ]] && { : </dev/tty; } 2>/dev/null; then
    printf '%s [%s]: ' "$__q" "$__def" > /dev/tty 2>/dev/null
    if IFS= read -r __ans < /dev/tty; then
      printf -v "$__var" '%s' "${__ans:-$__def}"
      return 0
    fi
  fi
  if ! read -r -p "$__q [$__def]: " __ans; then
    if [[ "$ASSUME_DEFAULTS" == "1" ]]; then
      printf '\n'
      info "No terminal — using default for \"$__q\": ${__def:-<empty>}"
      printf -v "$__var" '%s' "$__def"
      return 0
    fi
    printf '\n' >&2
    die "no terminal to read \"$__q\" from.
  Run setup from a terminal, or re-run with --yes to accept every default:
    cd $SCRIPT_DIR && ./setup.sh --yes"
  fi
  printf -v "$__var" '%s' "${__ans:-$__def}"
}

# Generate a URL-safe secret.
gen_secret() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 24
  else
    head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n'
  fi
}

# NOTE: the admin account is created by core itself at first boot, from OTW_ADMIN_USER /
# OTW_ADMIN_PASSWORD (written to .env below, passed through by compose). No HTTP call, no
# helper container, no network resolution — it always works, headless or not.

# True if a Postgres data volume from a previous install still exists. Its name is
# <project>_postgres-data. The project name comes from `name:` in docker-compose.yml
# (falling back to the lowercased dir name), NOT from .env. A surviving volume keeps the OLD
# database password, so pairing it with freshly generated secrets makes core fail with
# "password authentication failed" in a restart loop.
COMPOSE_PROJECT="$(
  awk -F: '/^[[:space:]]*name:[[:space:]]*/ {gsub(/[[:space:]]/,"",$2); print $2; exit}' \
    "$SCRIPT_DIR/docker-compose.yml" 2>/dev/null
)"
COMPOSE_PROJECT="${COMPOSE_PROJECT:-$(basename "$SCRIPT_DIR" | tr '[:upper:]' '[:lower:]')}"
pg_volume_exists() {
  docker volume inspect "${COMPOSE_PROJECT}_postgres-data" >/dev/null 2>&1
}

# List Docker volumes belonging to this compose project (empty if none / no docker).
project_volumes() {
  docker volume ls -q --filter "name=^${COMPOSE_PROJECT}_" 2>/dev/null
}

bold "OpenTraderWorld — setup"
echo

# Offer to remove leftover data volumes from a previous install before doing anything else.
# A surviving postgres volume keeps the OLD database password; pairing it with freshly
# generated secrets makes core crash-loop with "password authentication failed". Wiping here
# guarantees a clean slate. Declining keeps existing data (fine when reusing the same .env).
bold "0) Existing data volumes"
info "Compose project: ${COMPOSE_PROJECT}"
EXISTING_VOLS="$(project_volumes || true)"
if [[ -n "$EXISTING_VOLS" ]]; then
  info "Docker volumes from a previous install were found:"
  while IFS= read -r __v; do [[ -n "$__v" ]] && info "  - $__v"; done <<< "$EXISTING_VOLS"
  info "Wiping deletes all previous data (database, uploads) for a clean install."
  info "Keeping them only works if you also keep the previous .env (same DB password)."
  if [[ "$WIPE_VOLUMES" == "1" ]]; then
    info "--wipe-volumes given — removing without asking."
    CLEAN_VOLS=y
  else
    prompt_default CLEAN_VOLS "Remove these volumes before installing? (y/N)" "N"
  fi
  case "$CLEAN_VOLS" in
    [yY])
      info "Removing volumes…"
      # `down -v` tears down the project (containers + named volumes). Env files aren't
      # needed to remove volumes; they may not exist yet on a first run.
      ( cd "$SCRIPT_DIR" && docker compose -p "$COMPOSE_PROJECT" down -v --remove-orphans >/dev/null 2>&1 || true )
      # Drop any that survive (e.g. compose couldn't parse the file without env values).
      while IFS= read -r __v; do
        [[ -n "$__v" ]] && docker volume rm -f "$__v" >/dev/null 2>&1 || true
      done <<< "$EXISTING_VOLS"
      info "Done — starting from a clean slate."
      ;;
    *) info "Keeping existing volumes." ;;
  esac
else
  info "No existing volumes for this project — clean slate."
fi
echo

# Whether we (re)generate the secrets in .env this run. A fresh .env means a new
# POSTGRES_PASSWORD, which only takes effect on an *empty* database volume — so if secrets
# change we must also start from a clean volume (handled before `up`, below).
REGEN_SECRETS=1
if [[ -f "$ENV_FILE" ]]; then
  prompt_default OVERWRITE "An .env already exists. Overwrite it? (y/N)" "N"
  case "$OVERWRITE" in
    [yY]) ;;
    *) info "Keeping existing .env. Aborting setup."; exit 0 ;;
  esac
fi

# Best-effort LAN IPv4 of this host (macOS then Linux), empty if undetectable.
detect_lan_ip() {
  ipconfig getifaddr en0 2>/dev/null && return
  ipconfig getifaddr en1 2>/dev/null && return
  hostname -I 2>/dev/null | awk '{print $1}'
}

bold "1) Network"
info "How should the app be reachable? (changeable later in Settings → Network)"
info "  1) This machine only (localhost) — safest default"
info "  2) Local network (LAN), plain HTTP — some browsers force HTTPS and may not connect"
info "  3) Local network (LAN) + HTTPS — real certificate via DuckDNS (free), no browser"
info "     warnings on any device, nothing exposed to the internet"
info "  4) Public internet — reachable from anywhere at your own domain, HTTPS via"
info "     Let's Encrypt. Requires a public DNS record + ports 80/443 open inbound."
prompt_default NET_CHOICE "Choose 1-4" "1"

NETWORK_ENV="$SCRIPT_DIR/network.env"
DNS_ENV="$SCRIPT_DIR/dns.env"
OTW_ACME_GLOBAL=""
APP_URL=""

case "$NET_CHOICE" in
  4)
    info "Your domain must already resolve (A/AAAA record) to this server's PUBLIC IP,"
    info "and inbound TCP 80 + 443 must reach it (router/cloud firewall, port-forward)."
    info "Caddy obtains a Let's Encrypt certificate over HTTP-01 on first request."
    prompt_default OTW_DOMAIN "Public domain (e.g. app.example.com)" ""
    if [[ -z "$OTW_DOMAIN" || "$OTW_DOMAIN" == *" "* || "$OTW_DOMAIN" != *.* ]]; then
      echo "A public hostname like app.example.com is required." >&2; exit 1
    fi
    # `web` mode: all interfaces, HTTP→HTTPS on 80/443, no DNS token (HTTP-01, not DNS-01).
    OTW_BIND="0.0.0.0"; OTW_HTTPS_BIND="0.0.0.0"; OTW_HTTP_PORT="80"; OTW_HTTPS_PORT="443"
    OTW_ACME_GLOBAL=""
    APP_URL="https://${OTW_DOMAIN}"
    ;;
  3)
    info "Get a free subdomain + token at https://www.duckdns.org (sign in, add a domain)."
    prompt_default DUCK_DOMAIN "DuckDNS domain (yourname.duckdns.org)" ""
    prompt_default DUCK_TOKEN  "DuckDNS token" ""
    LAN_IP_GUESS="$(detect_lan_ip || true)"
    prompt_default LAN_IP "This machine's LAN IP" "${LAN_IP_GUESS:-192.168.1.20}"
    DUCK_SUB="${DUCK_DOMAIN%.duckdns.org}"
    if [[ -z "$DUCK_SUB" || "$DUCK_SUB" == "$DUCK_DOMAIN" || -z "$DUCK_TOKEN" ]]; then
      echo "A domain like yourname.duckdns.org and a token are required." >&2; exit 1
    fi
    info "Pointing ${DUCK_DOMAIN} at ${LAN_IP}…"
    DUCK_RES="$(curl -fsS "https://www.duckdns.org/update?domains=${DUCK_SUB}&token=${DUCK_TOKEN}&ip=${LAN_IP}" || true)"
    if [[ "$DUCK_RES" != "OK" ]]; then
      echo "DuckDNS rejected the update (got: ${DUCK_RES:-no response}) — check domain/token." >&2; exit 1
    fi
    OTW_BIND="0.0.0.0"; OTW_HTTPS_BIND="0.0.0.0"; OTW_HTTP_PORT="80"; OTW_HTTPS_PORT="443"
    OTW_DOMAIN="$DUCK_DOMAIN"
    OTW_ACME_GLOBAL="acme_dns duckdns ${DUCK_TOKEN}"
    APP_URL="https://${DUCK_DOMAIN}"
    ;;
  2)
    prompt_default OTW_HTTP_PORT "HTTP port" "5454"
    OTW_BIND="0.0.0.0"; OTW_HTTPS_BIND="127.0.0.1"; OTW_HTTPS_PORT="8443"; OTW_DOMAIN=":80"
    APP_URL="http://$(detect_lan_ip || echo localhost):${OTW_HTTP_PORT}"
    ;;
  *)
    prompt_default OTW_HTTP_PORT "HTTP port" "5454"
    OTW_BIND="127.0.0.1"; OTW_HTTPS_BIND="127.0.0.1"; OTW_HTTPS_PORT="8443"; OTW_DOMAIN=":80"
    APP_URL="http://localhost:${OTW_HTTP_PORT}"
    ;;
esac

# Network exposure lives in the secret-free network.env (rewritten later by Settings →
# Network); the ACME DNS line (holds the DuckDNS token in mode 3) lives in dns.env.
cat > "$NETWORK_ENV" <<EOF
# Generated by setup.sh — network exposure. Secret-free; safe to read.
# Edit via Settings → Network, then run: docker compose up -d
OTW_BIND=${OTW_BIND}
OTW_HTTPS_BIND=${OTW_HTTPS_BIND}
OTW_HTTP_PORT=${OTW_HTTP_PORT}
OTW_HTTPS_PORT=${OTW_HTTPS_PORT}
OTW_DOMAIN=${OTW_DOMAIN}
EOF
cat > "$DNS_ENV" <<EOF
# Generated by setup.sh — ACME DNS challenge for Caddy (LAN + HTTPS mode).
# Contains your DNS provider token when set: do not commit or share.
OTW_ACME_GLOBAL=${OTW_ACME_GLOBAL}
EOF

bold "2) Admin account"
info "Created now so the app is usable headlessly (no browser needed)."
prompt_default ADMIN_USER "Admin username" "admin"
# A strong random password, shown once below. Avoids typing a secret at a prompt and
# guarantees it is not weak. The operator changes it later in Settings if they wish.
ADMIN_PASS="$(gen_secret)"

bold "3) Logging"
prompt_default OTW_LOG "Log level (trace|debug|info|warn|error)" "info"

bold "4) Secrets"
info "Generating strong database & session secrets…"
POSTGRES_USER="otw"
POSTGRES_DB="opentraderworld"
POSTGRES_PASSWORD="$(gen_secret)"
SESSION_SECRET="$(gen_secret)"
# Master key for encrypting feed secrets (API keys/tokens) at rest. Changing it
# later makes existing stored secrets unreadable — keep it stable.
OTW_SECRET_KEY="$(gen_secret)"
DATABASE_URL="postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@postgres:5432/${POSTGRES_DB}"

umask 077
cat > "$ENV_FILE" <<EOF
# Generated by setup.sh on $(date -u +%Y-%m-%dT%H:%M:%SZ). Do not commit.

# ── Core ──
OTW_CORE_HOST=0.0.0.0
OTW_CORE_PORT=8080
OTW_LOG=${OTW_LOG}

# ── PostgreSQL ──
POSTGRES_USER=${POSTGRES_USER}
POSTGRES_PASSWORD=${POSTGRES_PASSWORD}
POSTGRES_DB=${POSTGRES_DB}
DATABASE_URL=${DATABASE_URL}

# ── Auth ──
SESSION_SECRET=${SESSION_SECRET}

# ── Secrets encryption (feed API keys/tokens) ──
# Master key for AEAD encryption at rest. Keep stable; rotating invalidates
# previously stored feed secrets.
OTW_SECRET_KEY=${OTW_SECRET_KEY}

# ── Community Docs submissions ──
# OPTIONAL trusted-instance token for relaying editor doc submissions to the review
# website. Blank is fine: anonymous submissions are accepted (rate-limited, reviewed).
DOC_SUBMISSION_TOKEN=

# ── First-boot admin ──
# Core creates this admin on first boot if none exists yet (headless install). Safe to
# leave in place: it is a no-op once the account exists. Change the password in-app after
# first login. Remove these lines to fall back to the browser wizard.
OTW_ADMIN_USER=${ADMIN_USER}
OTW_ADMIN_PASSWORD=${ADMIN_PASS}
EOF

echo
bold "Wrote $ENV_FILE (permissions 600)."
info "Secrets were generated automatically and kept out of git."
echo
# Network exposure (bind/port/domain) lives in the secret-free network.env, managed in-app
# from Settings → Network. Both env files must be passed so compose interpolates the ports.
#
# Default: layer docker-compose.images.yml so compose PULLS the prebuilt images from Docker
# Hub (no Rust/Node toolchain needed). With --build we omit the override and `up --build`
# compiles the three services from source instead (for development / local changes).
if [[ "$BUILD_FROM_SOURCE" == "1" ]]; then
  COMPOSE="docker compose --env-file .env --env-file network.env"
  UP_ARGS="--build -d"
  info "Install mode: build from source (--build)."
else
  COMPOSE="docker compose -f docker-compose.yml -f docker-compose.images.yml --env-file .env --env-file network.env"
  UP_ARGS="-d"
  info "Install mode: pull prebuilt images from Docker Hub (run with --build to build from source)."
fi

prompt_default START_NOW "Bring the stack up now? (Y/n)" "Y"
case "$START_NOW" in
  [nN])
    info "Skipped. Start later with: cd deploy && $COMPOSE up $UP_ARGS"
    info "Then create the admin in the browser wizard at ${APP_URL}."
    ;;
  *)
    # Guard the stale-volume trap: fresh secrets + a leftover database volume = old password,
    # which makes core crash-loop with "password authentication failed". Offer to wipe.
    if [[ "$REGEN_SECRETS" == "1" ]] && pg_volume_exists; then
      echo
      info "A database volume from a previous install exists (${COMPOSE_PROJECT}_postgres-data)."
      info "It still holds the OLD database password, which won't match the freshly generated"
      info "secrets — core would fail to start. A clean install must start from an empty volume."
      if [[ "$WIPE_VOLUMES" == "1" ]]; then
        WIPE_VOL=y
      else
        prompt_default WIPE_VOL "Wipe existing data volumes for a clean install? (y/N)" "N"
      fi
      case "$WIPE_VOL" in
        [yY])
          info "Removing old volumes…"
          ( cd "$SCRIPT_DIR" && $COMPOSE down -v --remove-orphans >/dev/null 2>&1 || true )
          ;;
        *)
          echo "Keeping the old volume — core will likely fail auth. Aborting to avoid a" >&2
          echo "broken stack. To start clean (DELETES the previous database and uploads):" >&2
          echo "    cd $SCRIPT_DIR && ./setup.sh --wipe-volumes" >&2
          echo "To keep your data instead, restore the previous .env and re-run ./setup.sh." >&2
          exit 1
          ;;
      esac
    fi
    bold "Starting the stack…"
    ( cd "$SCRIPT_DIR" && $COMPOSE up $UP_ARGS )
    echo
    # Core creates the admin on first boot from OTW_ADMIN_USER/PASSWORD in .env — nothing
    # to do here but show the credentials.
    bold "Admin account (created by core on first boot). Save these — password shown once:"
    info "  username: ${ADMIN_USER}"
    info "  password: ${ADMIN_PASS}"
    echo
    bold "Up. Open ${APP_URL} and sign in."
    case "$NET_CHOICE" in
      3|4) info "The first HTTPS request can take ~30 s while the certificate is issued." ;;
    esac
    if [[ "$NET_CHOICE" == "4" ]]; then
      info "The app is now PUBLIC. Ensure ports 80/443 are open and keep the admin password safe."
    fi
    info "Network exposure can be changed anytime in Settings → Network (or see the Network docs to change it from the CLI)."
    ;;
esac
