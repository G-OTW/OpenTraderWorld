#!/usr/bin/env bash
#
# OpenTraderWorld — server setup, for a machine you already rent under a domain
# name you already own.
#
#   curl -fsSL https://get.opentraderworld.com/configure_install.sh | bash -s -- --domain app.example.com
#
# Run it on a fresh Debian 13 or Ubuntu 24.04 machine, signed in either as root
# or as the account your provider gave you — it works out which, and asks for
# administrator rights itself if it needs them. It leaves behind a public,
# hardened, HTTPS instance. Four phases, in this order:
#
#   check    is the machine usable, and does the domain already point here
#   harden   day-to-day account, key-only remote access, firewall, jail,
#            automatic security updates, swap, Docker, bounded logs
#   install  fetch deploy/ at the pinned ref and run setup.sh in public mode
#   cert     wait for the certificate and the first successful HTTPS reply
#   backup   the otw command, a nightly backup, taken and checked once here
#
# Each phase records itself in /var/lib/otw/install-state, and the answers are
# kept in /var/lib/otw/install.conf. Re-running the same line skips what already
# succeeded and resumes at the failure, with no flags to repeat — that is the
# only recovery instruction the docs ever give (ProjectSpecs/hosting/plan.md R4).
#
# Anything it has not been told, it asks for, one question at a time, in plain
# words. Answer nothing and every question takes the sensible default.
#
# Usage:
#   server-setup.sh --domain app.example.com --login key|password [options]
#
# Options:
#   --domain <name>     Public domain that already points at this machine (asked if missing)
#   --login key         Sign in from now on with the key you already use
#   --login password    Sign in from now on with a password (asked if missing)
#   --user <name>       Day-to-day account                     (default: the one in use, else otw)
#   --ssh-key <key>     Public key to authorise, repeatable     (default: the ones already trusted)
#   --ssh-port <n>      Port the machine already listens on     (default: from sshd, else 22)
#   --dir <path>        Install directory                       (default /home/<user>/otw)
#   --ref <ref>         Product tag or branch to install        (default master)
#   --repo <slug>       GitHub repo slug                        (default G-OTW/OpenTraderWorld)
#   --timezone <tz>     Machine timezone, e.g. Europe/Paris     (default: leave as is)
#   --swap <size>       Swap file size, e.g. 2G, or "none"      (default: 2G under 4 GB of RAM)
#   --yes               Never ask anything; take every default
#   --only <phase>      Run one phase only (check|harden|install|cert), repeatable
#   --skip <phase>      Skip one phase, repeatable
#   --redo              Ignore the state file and run every phase again
#   --dry-run           Print what would change, change nothing
#   -h, --help          Show this help
#
# --only, --skip, --redo and --dry-run are support tools. They are deliberately
# absent from the user-facing documentation: the published instruction is one
# line, and re-running it is the fix for everything.
set -euo pipefail

# Kept before the options are parsed and shifted away, because asking the machine
# for administrator rights means starting this script again — and starting it
# again with half its arguments missing is worse than not starting it at all.
ORIG_ARGS=("$@")

VERSION="1"
REPO="${OTW_REPO:-G-OTW/OpenTraderWorld}"
REF="${OTW_REF:-master}"
STATE_DIR="/var/lib/otw"
STATE_FILE="$STATE_DIR/install-state"
CONF_FILE="$STATE_DIR/install.conf"
ALL_PHASES=(check harden install cert backup)

DOMAIN=""
OTW_USER=""
DIR=""
SSH_PORT=""
TIMEZONE=""
SWAP_SIZE=""
# "key" or "password": how the operator signs in to the machine from now on.
# Empty means nobody has said yet, and the question gets asked.
LOGIN_MODE=""
# Set to 0 by --yes, and by the absence of any terminal to ask through.
INTERACTIVE=1
DRY_RUN=0
REDO=0
SSH_KEYS=()
ONLY=()
SKIP=()

# ─────────────────────────────────────────────────────────────────────────────
# Output. Everything the operator reads is a plain sentence: no jargon on the
# main path, and every failure names the exact next action (plan.md R5/G8).
# ─────────────────────────────────────────────────────────────────────────────
bold()  { printf '\033[1m%s\033[0m\n' "$1"; }
info()  { printf '  %s\n' "$1"; }
warn()  { printf '  ! %s\n' "$1"; }
step()  { printf '\n\033[1m%s\033[0m\n' "$1"; }

# Stops the run with one sentence plus what to do about it. The state file keeps
# the phases that already succeeded, so the next run resumes here.
die() {
  printf '\n\033[1mStopped.\033[0m\n' >&2
  while [[ $# -gt 0 ]]; do printf '  %s\n' "$1" >&2; shift; done
  printf '\n  Nothing else on this machine was changed.\n' >&2
  printf '  Fix the point above, then run the same line again — it carries on from here.\n\n' >&2
  exit 1
}

usage() { sed -n '2,/^set -euo/p' "$0" | sed 's/^# \{0,1\}//; $d'; }

# ── Questions ────────────────────────────────────────────────────────────────
# The line is pasted as `curl … | bash`, so this script's own stdin is the spent
# download, not the keyboard. Every question is therefore read from /dev/tty,
# which is still the operator's terminal. Where there is no terminal at all
# (a provider's boot script, CI), every question silently takes its default, so
# nothing can ever hang waiting for an answer nobody can give.
tty_ok() {
  [[ "$INTERACTIVE" == "1" ]] || return 1
  { : </dev/tty; } 2>/dev/null
}

# ask VAR "Question" ["default"]
ask() {
  local __var="$1" __q="$2" __def="${3:-}" __ans=""
  if ! tty_ok; then
    printf -v "$__var" '%s' "$__def"
    return 0
  fi
  if [[ -n "$__def" ]]; then
    printf '  %s [%s]: ' "$__q" "$__def" > /dev/tty
  else
    printf '  %s: ' "$__q" > /dev/tty
  fi
  IFS= read -r __ans < /dev/tty || __ans=""
  printf -v "$__var" '%s' "${__ans:-$__def}"
}

# confirm "Question" [y|n] — true on yes. Without a terminal, the default wins.
confirm() {
  local __q="$1" __def="${2:-y}" __a=""
  if ! tty_ok; then [[ "$__def" == "y" ]]; return; fi
  ask __a "$__q (yes/no)" "$__def"
  [[ "$__a" =~ ^[yYoO] ]]
}

# Runs a command, or prints it under --dry-run. Used for everything that writes
# outside this script's own temporary files.
run() {
  if [[ "$DRY_RUN" == "1" ]]; then
    printf '  would run: %s\n' "$*"
    return 0
  fi
  "$@"
}

# Writes a file, or prints its path and body under --dry-run. Body on stdin.
write_file() {
  local path="$1" mode="${2:-0644}"
  if [[ "$DRY_RUN" == "1" ]]; then
    printf '  would write %s (mode %s):\n' "$path" "$mode"
    sed 's/^/    | /'
    return 0
  fi
  install -D -m "$mode" /dev/null "$path"
  cat > "$path"
}

# ─────────────────────────────────────────────────────────────────────────────
# Arguments
# ─────────────────────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --domain)   [[ $# -ge 2 ]] || die "--domain needs a name, like app.example.com"; DOMAIN="$2"; shift 2 ;;
    --user)     [[ $# -ge 2 ]] || die "--user needs a name"; OTW_USER="$2"; shift 2 ;;
    --ssh-key)  [[ $# -ge 2 ]] || die "--ssh-key needs a public key"; SSH_KEYS+=("$2"); shift 2 ;;
    --ssh-port) [[ $# -ge 2 ]] || die "--ssh-port needs a number"; SSH_PORT="$2"; shift 2 ;;
    --dir)      [[ $# -ge 2 ]] || die "--dir needs a path"; DIR="$2"; shift 2 ;;
    --ref)      [[ $# -ge 2 ]] || die "--ref needs a tag or branch"; REF="$2"; shift 2 ;;
    --repo)     [[ $# -ge 2 ]] || die "--repo needs a slug"; REPO="$2"; shift 2 ;;
    --timezone) [[ $# -ge 2 ]] || die "--timezone needs a zone, like Europe/Paris"; TIMEZONE="$2"; shift 2 ;;
    --swap)     [[ $# -ge 2 ]] || die "--swap needs a size, like 2G, or none"; SWAP_SIZE="$2"; shift 2 ;;
    --login)    [[ $# -ge 2 ]] || die "--login needs either key or password"
                case "$2" in
                  key|password) LOGIN_MODE="$2" ;;
                  *) die "--login takes either key or password, not \"$2\"." ;;
                esac
                shift 2 ;;
    --keep-password-login) LOGIN_MODE="password"; shift ;;
    -y|--yes)   INTERACTIVE=0; shift ;;
    --only)     [[ $# -ge 2 ]] || die "--only needs a phase name"; ONLY+=("$2"); shift 2 ;;
    --skip)     [[ $# -ge 2 ]] || die "--skip needs a phase name"; SKIP+=("$2"); shift 2 ;;
    --redo)     REDO=1; shift ;;
    --dry-run)  DRY_RUN=1; shift ;;
    -h|--help)  usage; exit 0 ;;
    *) die "I do not know the option \"$1\"." "Run it with --help to see the ones that exist." ;;
  esac
done

# ─────────────────────────────────────────────────────────────────────────────
# Machine facts, gathered once
# ─────────────────────────────────────────────────────────────────────────────
# Signed in as an ordinary account: ask the machine for administrator rights and
# start again with them, rather than failing halfway through. Providers hand out
# both kinds of machine, and which one you got is not something to have to know.
if [[ "$(id -u)" != "0" ]]; then
  command -v sudo >/dev/null 2>&1 || die \
    "This needs the machine's administrator rights, and this account cannot ask for them." \
    "Sign in to your machine again as root, then run the same line."
  if [[ -f "$0" && -r "$0" ]]; then
    printf '\n  This needs the machine administrator rights.\n'
    printf '  If it asks for a password, type the one you use for this machine.\n\n'
    exec sudo -p "  Password for %p: " bash "$0" ${ORIG_ARGS+"${ORIG_ARGS[@]}"}
  fi
  die "This needs the machine's administrator rights." \
      "Run the same line with sudo in the middle, exactly like this:" \
      "  curl -fsSL https://get.opentraderworld.com/configure_install.sh | sudo bash -s -- --domain $DOMAIN"
fi

# The account the operator was signed in as before sudo. It already exists, it
# already has a way in that works, and it is the one they will keep using — so
# it is the natural owner of the install, not a second account they never asked
# for. Only a machine entered straight as root gets a new account made for it.
INVOKING_USER="${SUDO_USER:-root}"

OS_ID=""; OS_LIKE=""; OS_VER=""; OS_NAME=""; OS_CODENAME=""
if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  OS_ID="${ID:-}"; OS_LIKE="${ID_LIKE:-}"; OS_VER="${VERSION_ID:-}"; OS_NAME="${PRETTY_NAME:-$ID}"
  OS_CODENAME="${UBUNTU_CODENAME:-${VERSION_CODENAME:-}}"
fi
ARCH="$(uname -m)"

# Re-runs must not ask again for anything (plan.md R3): the previous answers are
# on the machine, and a flag given today wins over the file.
if [[ -r "$CONF_FILE" ]]; then
  # shellcheck disable=SC1090
  saved_domain=""; saved_user=""; saved_dir=""; saved_ssh_port=""; saved_ref=""; saved_repo=""; saved_login=""
  # shellcheck disable=SC1091
  . "$CONF_FILE"
  DOMAIN="${DOMAIN:-$saved_domain}"
  OTW_USER="${OTW_USER:-$saved_user}"
  DIR="${DIR:-$saved_dir}"
  SSH_PORT="${SSH_PORT:-$saved_ssh_port}"
  LOGIN_MODE="${LOGIN_MODE:-$saved_login}"
  [[ "$REF" == "master" && -n "$saved_ref" ]] && REF="$saved_ref"
  [[ "$REPO" == "G-OTW/OpenTraderWorld" && -n "$saved_repo" ]] && REPO="$saved_repo"
fi

# Keep the account already in use; make one called otw only when the machine was
# entered straight as root and there is nothing to keep.
if [[ -z "$OTW_USER" ]]; then
  if [[ "$INVOKING_USER" != "root" ]] && id -u "$INVOKING_USER" >/dev/null 2>&1; then
    OTW_USER="$INVOKING_USER"
  else
    OTW_USER="otw"
  fi
fi
DIR="${DIR:-/home/$OTW_USER/otw}"

# What to type in the "name" box at the registrar. example.com is the root of
# the zone — registrars print that as @, or leave the box empty — while
# app.example.com is the single word "app". Getting this wrong is the single
# most common way the address never works.
record_name() {
  local d="$1"
  if [[ "$(printf '%s' "$d" | tr -cd . | wc -c)" -le 1 ]]; then
    printf '@  (some sites call it the root, or want the box left empty)\n'
  else
    printf '%s\n' "${d%%.*}"
  fi
}

# A hostname, not a URL and not an address made of numbers: neither of those can
# be given a certificate. Cleaned rather than refused — people paste what is in
# the browser bar, with https:// and a slash on the end.
clean_domain() {
  local d="${1:-}"
  d="${d#http://}"; d="${d#https://}"; d="${d%%/*}"
  d="${d%.}"
  printf '%s\n' "$d" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]'
}
domain_ok() {
  local d="${1:-}"
  [[ "$d" == *.* && "$d" != *" "* && ! "$d" =~ ^[0-9.]+$ ]]
}

# ─────────────────────────────────────────────────────────────────────────────
# The two questions, asked only when the line did not already answer them
# ─────────────────────────────────────────────────────────────────────────────
bold "OpenTraderWorld"
info "This puts OpenTraderWorld online on this machine, at your own address."
echo

DOMAIN="$(clean_domain "$DOMAIN")"
while ! domain_ok "$DOMAIN"; do
  if [[ -n "$DOMAIN" ]]; then
    warn "\"$DOMAIN\" is not a name that can be used. It looks like app.example.com."
  fi
  tty_ok || die \
    "I need the domain name people will type to reach this machine." \
    "Add it to the line, like this:" \
    "  ... | bash -s -- --domain app.example.com"
  ask DOMAIN "The address people will type to reach you (for example app.example.com)"
  DOMAIN="$(clean_domain "$DOMAIN")"
done

# How they will get back into this machine afterwards. Asked before anything is
# touched, because it is the one decision that can lock somebody out of their own
# server, and it is not one to discover halfway through.
if [[ -z "$LOGIN_MODE" ]]; then
  if [[ ${#SSH_KEYS[@]} -gt 0 ]] || [[ -s /root/.ssh/authorized_keys ]] \
     || { [[ "$INVOKING_USER" != "root" ]] && [[ -s "$(getent passwd "$INVOKING_USER" | cut -d: -f6 || true)/.ssh/authorized_keys" ]]; }; then
    LOGIN_MODE="key"
  else
    LOGIN_MODE="password"
  fi
  if tty_ok; then
    echo
    info "How do you want to get back into this machine from now on?"
    info "  1) With the key you already use — nothing new to remember, and the safest."
    info "  2) With a password, the way you signed in today."
    local_choice=""
    ask local_choice "Type 1 or 2" "$([[ "$LOGIN_MODE" == "key" ]] && echo 1 || echo 2)"
    case "$local_choice" in
      1) LOGIN_MODE="key" ;;
      2) LOGIN_MODE="password" ;;
      *) warn "Not 1 or 2 — keeping the usual answer for this machine." ;;
    esac
  fi
fi

if [[ ! "$OTW_USER" =~ ^[a-z_][a-z0-9_-]{0,31}$ ]]; then
  die "\"$OTW_USER\" cannot be used as an account name." \
      "Use lowercase letters, digits, - and _, starting with a letter."
fi

# ─────────────────────────────────────────────────────────────────────────────
# State
# ─────────────────────────────────────────────────────────────────────────────
phase_done() {
  [[ "$REDO" == "1" ]] && return 1
  [[ -r "$STATE_FILE" ]] && grep -qx "$1=done" "$STATE_FILE"
}

mark_done() {
  [[ "$DRY_RUN" == "1" ]] && return 0
  mkdir -p "$STATE_DIR"; chmod 700 "$STATE_DIR"
  touch "$STATE_FILE"; chmod 600 "$STATE_FILE"
  local tmp; tmp="$(mktemp)"
  grep -v "^$1=" "$STATE_FILE" > "$tmp" || true
  printf '%s=done\n' "$1" >> "$tmp"
  cat "$tmp" > "$STATE_FILE"
  rm -f "$tmp"
}

save_conf() {
  [[ "$DRY_RUN" == "1" ]] && return 0
  mkdir -p "$STATE_DIR"; chmod 700 "$STATE_DIR"
  cat > "$CONF_FILE" <<CONF
# Written by server-setup.sh — the answers a re-run must not ask for again.
saved_domain="$DOMAIN"
saved_user="$OTW_USER"
saved_dir="$DIR"
saved_ssh_port="$SSH_PORT"
saved_login="$LOGIN_MODE"
saved_ref="$REF"
saved_repo="$REPO"
saved_version="$VERSION"
CONF
  chmod 600 "$CONF_FILE"
}

wanted() {
  local phase="$1" p
  if [[ ${#ONLY[@]} -gt 0 ]]; then
    for p in "${ONLY[@]}"; do [[ "$p" == "$phase" ]] && return 0; done
    return 1
  fi
  for p in ${SKIP+"${SKIP[@]}"}; do [[ "$p" == "$phase" ]] && return 1; done
  return 0
}

# ─────────────────────────────────────────────────────────────────────────────
# check — is this machine usable, and is the domain already pointing at it
# ─────────────────────────────────────────────────────────────────────────────

# Every address this machine answers on, one per line. A machine behind a
# provider NAT sees only its private address locally, so the public one is asked
# for separately below and both are accepted.
local_addresses() {
  { ip -o addr show scope global 2>/dev/null || true; } | awk '{print $4}' | cut -d/ -f1
}

public_address() {
  local url ip
  for url in https://api.ipify.org https://ifconfig.me/ip https://icanhazip.com; do
    ip="$(curl -fsS4 --max-time 8 "$url" 2>/dev/null | tr -d '[:space:]')" || continue
    [[ "$ip" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] && { printf '%s\n' "$ip"; return 0; }
  done
  return 1
}

# A records for the domain, without depending on dig being installed.
domain_addresses() {
  getent ahostsv4 "$DOMAIN" 2>/dev/null | awk '{print $1}' | sort -u
}

phase_check() {
  step "1/4  Checking this machine"

  case "$OS_ID $OS_LIKE" in
    *debian*|*ubuntu*) : ;;
    *) die "This installer only knows Ubuntu and Debian, and this machine runs ${OS_NAME:-an unknown system}." \
           "Reinstall the machine with Debian 13 or Ubuntu 24.04 at your provider, then run the same line again." ;;
  esac
  command -v apt-get >/dev/null 2>&1 || die \
    "This machine has no apt, so it is not the Ubuntu or Debian this installer needs." \
    "Reinstall it with Debian 13 or Ubuntu 24.04 at your provider, then run the same line again."
  info "System: ${OS_NAME:-unknown} ($ARCH)"

  case "$ARCH" in
    x86_64|amd64|aarch64|arm64) : ;;
    *) die "OpenTraderWorld does not have images for $ARCH processors." \
           "Pick a machine described as x86 / AMD64, or as ARM64 / Ampere." ;;
  esac

  local mem_kb mem_mb
  mem_kb="$(awk '/^MemTotal:/ {print $2}' /proc/meminfo)"
  mem_mb=$(( mem_kb / 1024 ))
  info "Memory: ${mem_mb} MB"
  if (( mem_mb < 1700 )); then
    die "This machine has ${mem_mb} MB of memory, and OpenTraderWorld needs about 2 GB." \
        "At your provider, resize it to a plan with 2 GB or more, then run the same line again."
  fi
  (( mem_mb < 3500 )) && warn "Under 4 GB of memory: a swap file will be added so heavy work cannot kill the database."

  local free_mb
  free_mb="$(df -Pm / | awk 'NR==2 {print $4}')"
  info "Free disk: ${free_mb} MB"
  if (( free_mb < 8000 )); then
    die "Only ${free_mb} MB of disk space is free, and the install needs about 8 GB." \
        "At your provider, give the machine a bigger disk, then run the same line again."
  fi

  curl -fsS --max-time 15 -o /dev/null https://github.com 2>/dev/null || die \
    "This machine cannot reach the internet." \
    "Check at your provider that the machine is running and has an address, then run the same line again."
  info "Internet: reachable"

  # The domain is the one thing this script cannot do for the operator, so the
  # message names the record, the value and the place to type it (plan.md §4.5).
  local pub addrs
  pub="$(public_address || true)"
  addrs="$(domain_addresses || true)"
  if [[ -z "$addrs" ]]; then
    die "The name $DOMAIN does not lead anywhere yet." \
        "Go to the company you bought $DOMAIN from, open its DNS or Zone page, and add:" \
        "    type A     name $(record_name "$DOMAIN")     value ${pub:-the address of this machine}" \
        "Wait five minutes, then run the same line again."
  fi
  info "$DOMAIN points to: $(echo "$addrs" | tr '\n' ' ')"
  if [[ -n "$pub" ]]; then
    info "This machine's address: $pub"
    local match=0 a
    for a in $addrs; do
      [[ "$a" == "$pub" ]] && match=1
      local l; for l in $(local_addresses); do [[ "$a" == "$l" ]] && match=1; done
    done
    if (( match == 0 )) && tty_ok; then
      # A name pointed at the machine minutes ago is simply not known everywhere
      # yet. Waiting here is kinder than sending someone away to come back later,
      # and it is the single most common reason this step fails.
      warn "The name $DOMAIN does not lead to this machine yet."
      if confirm "Wait here while it spreads across the internet? It can take a few minutes." y; then
        local waited=0
        while (( waited < 600 )); do
          sleep 20; waited=$(( waited + 20 ))
          printf '  still waiting… %s min\n' "$(( waited / 60 ))"
          addrs="$(domain_addresses || true)"
          for a in $addrs; do [[ "$a" == "$pub" ]] && match=1; done
          (( match == 1 )) && break
        done
      fi
    fi
    if (( match == 0 )); then
      die "The name $DOMAIN does not lead to this machine." \
          "Go to the company you bought $DOMAIN from, open its DNS or Zone page, and set:" \
          "    type A     name $(record_name "$DOMAIN")     value $pub" \
          "Remove any other A record for that name. Wait five minutes, then run the same line again."
    fi
    info "The name and the machine agree."
  else
    warn "Could not confirm this machine's public address; carrying on with the name's own answer."
  fi

  # Ports 80 and 443 have to be free for the certificate. A listener that is not
  # ours is a web server the provider image left running.
  if ! phase_done install; then
    local busy=""
    if command -v ss >/dev/null 2>&1; then
      busy="$(ss -lntH 2>/dev/null | awk '{print $4}' | grep -Eo ':(80|443)$' | sort -u | tr -d ':' | tr '\n' ' ' || true)"
    fi
    if [[ -n "${busy// /}" ]]; then
      die "Something on this machine is already answering on port ${busy% }." \
          "That is usually a web server the provider installed. Turn it off with:" \
          "    systemctl disable --now apache2 nginx caddy 2>/dev/null; true" \
          "Then run the same line again."
    fi
  fi

  info "This machine is ready."
}

# ─────────────────────────────────────────────────────────────────────────────
# harden — the machine itself, before anything is exposed
# ─────────────────────────────────────────────────────────────────────────────

apt_quiet() {
  DEBIAN_FRONTEND=noninteractive run apt-get -o Dpkg::Use-Pty=0 -qq "$@"
}

APT_UPDATED=0
apt_ensure() {
  local missing=() p
  for p in "$@"; do dpkg -s "$p" >/dev/null 2>&1 || missing+=("$p"); done
  [[ ${#missing[@]} -eq 0 ]] && return 0
  if (( APT_UPDATED == 0 )); then
    info "Refreshing the list of available updates…"
    apt_quiet update || die "This machine could not download its update list." \
      "Check that it still has internet at your provider, then run the same line again."
    APT_UPDATED=1
  fi
  info "Installing: ${missing[*]}"
  apt_quiet install -y --no-install-recommends "${missing[@]}" \
    || die "Could not install ${missing[*]}." "Run the same line again; if it repeats, the machine's software sources are broken."
}

# Port sshd actually listens on, so the firewall never closes the door we came in
# through. Falls back to 22, which is what every provider image uses.
detect_ssh_port() {
  local p
  p="$(awk '/^[[:space:]]*Port[[:space:]]+[0-9]+/ {print $2; exit}' /etc/ssh/sshd_config 2>/dev/null || true)"
  if [[ -z "$p" && -d /etc/ssh/sshd_config.d ]]; then
    p="$(awk '/^[[:space:]]*Port[[:space:]]+[0-9]+/ {print $2; exit}' /etc/ssh/sshd_config.d/*.conf 2>/dev/null || true)"
  fi
  printf '%s\n' "${p:-22}"
}

harden_account() {
  if id -u "$OTW_USER" >/dev/null 2>&1; then
    info "Using the account you are already signed in with: \"$OTW_USER\"."
  else
    info "Creating your day-to-day account, \"$OTW_USER\"."
    run useradd --create-home --shell /bin/bash "$OTW_USER"
    # Signing in by password with an account that has none is impossible, and a
    # password nobody was told is the same as no account at all. So it is made
    # here and printed on the card at the end, once, like the app password.
    if [[ "$LOGIN_MODE" == "password" ]]; then
      ACCOUNT_PASS="$(gen_secret)"
      if [[ "$DRY_RUN" != "1" ]]; then
        printf '%s:%s\n' "$OTW_USER" "$ACCOUNT_PASS" | chpasswd \
          || die "Could not give the new account a password." "Run the same line again."
      fi
      info "It has been given a password, printed on the card at the end."
    fi
  fi

  # Administrator rights without a password prompt, which is what every cloud
  # image already does for its own first account: the account never has a
  # password to remember, and nothing here can ask for one (plan.md R3).
  write_file /etc/sudoers.d/90-otw 0440 <<SUDOERS
# Written by server-setup.sh. The install account administers this machine.
$OTW_USER ALL=(ALL) NOPASSWD:ALL
SUDOERS
  [[ "$DRY_RUN" == "1" ]] || visudo -cf /etc/sudoers.d/90-otw >/dev/null \
    || { rm -f /etc/sudoers.d/90-otw; die "The administrator rights file was rejected and has been removed." "Run the same line again."; }
}

harden_keys() {
  local home ak count=0 key
  home="$(getent passwd "$OTW_USER" | cut -d: -f6 || true)"
  ak="$home/.ssh/authorized_keys"

  # Keys given on the command line first, then whatever root already trusts —
  # that second set is how the operator is signed in right now, so copying it
  # guarantees the new account is reachable before anything is locked down.
  local collected=()
  for key in ${SSH_KEYS+"${SSH_KEYS[@]}"}; do
    [[ -n "${key// /}" ]] && collected+=("$key")
  done
  if [[ -r /root/.ssh/authorized_keys ]]; then
    while IFS= read -r line; do
      [[ -z "${line// /}" || "$line" == \#* ]] && continue
      collected+=("$line")
    done < /root/.ssh/authorized_keys
  fi
  if [[ -r "$ak" ]]; then
    while IFS= read -r line; do
      [[ -z "${line// /}" || "$line" == \#* ]] && continue
      collected+=("$line")
    done < "$ak"
  fi

  # The account the operator came in through: its key is the proof that the way
  # back in already works, which is what makes closing the password door safe.
  if [[ "$INVOKING_USER" != "root" && "$INVOKING_USER" != "$OTW_USER" ]]; then
    local invoker_ak
    invoker_ak="$(getent passwd "$INVOKING_USER" | cut -d: -f6 || true)/.ssh/authorized_keys"
    if [[ -r "$invoker_ak" ]]; then
      while IFS= read -r line; do
        [[ -z "${line// /}" || "$line" == \#* ]] && continue
        collected+=("$line")
      done < "$invoker_ak"
    fi
  fi

  if [[ ${#collected[@]} -gt 0 ]]; then
    if [[ "$DRY_RUN" == "1" ]]; then
      info "would authorise ${#collected[@]} key(s) for $OTW_USER"
      count=${#collected[@]}
    else
      install -d -m 700 -o "$OTW_USER" -g "$OTW_USER" "$home/.ssh"
      printf '%s\n' "${collected[@]}" | awk '!seen[$0]++' > "$ak"
      chmod 600 "$ak"; chown "$OTW_USER:$OTW_USER" "$ak"
      count="$(wc -l < "$ak" | tr -d ' ')"
    fi
    info "The account \"$OTW_USER\" can be reached with $count sign-in key(s)."
    HAVE_KEYS=1
  else
    HAVE_KEYS=0
  fi
}

harden_ssh() {
  local drop=/etc/ssh/sshd_config.d/99-otw.conf pw_line root_line
  if [[ ! -e /etc/ssh/sshd_config ]]; then
    warn "This machine has no remote-access service, so there is nothing to tighten there."
    return 0
  fi

  # The door is closed only against something already proven to work. Asking for
  # keys and finding none is not a reason to lock the operator out of their own
  # machine — it is a reason to say so and leave the password door open.
  if [[ "$LOGIN_MODE" == "key" ]] && (( HAVE_KEYS == 0 )); then
    warn "You chose to sign in with your key, but this machine trusts no key yet."
    warn "Sign-in by password stays on, so you are not locked out."
    warn "Put your key on the machine, then run the same line again to close it."
    LOGIN_MODE="password"
  fi

  if [[ "$LOGIN_MODE" == "key" ]]; then
    pw_line="PasswordAuthentication no"
    root_line="PermitRootLogin prohibit-password"
    info "From now on this machine only opens to your key. Passwords are refused."
  else
    pw_line="PasswordAuthentication yes"
    # Whatever happens, root stops being reachable from the outside: it is the
    # one account name every attacker on the internet already knows.
    root_line="PermitRootLogin no"
    info "From now on you sign in as \"$OTW_USER\" with your password. Direct root sign-in is refused."
  fi

  local body
  body="$(cat <<SSHD
# Written by server-setup.sh. Remove this file to go back to the settings this machine came with.
$root_line
$pw_line
KbdInteractiveAuthentication no
ChallengeResponseAuthentication no
PermitEmptyPasswords no
X11Forwarding no
MaxAuthTries 4
LoginGraceTime 30
ClientAliveInterval 300
ClientAliveCountMax 2
SSHD
)"

  if grep -qE '^[[:space:]]*Include[[:space:]]+/etc/ssh/sshd_config\.d/' /etc/ssh/sshd_config 2>/dev/null; then
    printf '%s\n' "$body" | write_file "$drop" 0600
  else
    # Older images without the include directory: append one marked block, and
    # replace it rather than stacking a new copy on every run.
    if [[ "$DRY_RUN" == "1" ]]; then
      info "would append the same settings to /etc/ssh/sshd_config"
    else
      sed -i '/^# >>> OpenTraderWorld >>>$/,/^# <<< OpenTraderWorld <<<$/d' /etc/ssh/sshd_config
      { echo "# >>> OpenTraderWorld >>>"; printf '%s\n' "$body"; echo "# <<< OpenTraderWorld <<<"; } >> /etc/ssh/sshd_config
    fi
  fi

  if [[ "$DRY_RUN" != "1" ]]; then
    if ! sshd -t 2>/dev/null; then
      rm -f "$drop"
      sed -i '/^# >>> OpenTraderWorld >>>$/,/^# <<< OpenTraderWorld <<<$/d' /etc/ssh/sshd_config
      die "The remote-access settings were refused by the machine and have been undone." \
          "Nothing was changed. Run the same line again."
    fi
    systemctl reload ssh 2>/dev/null || systemctl reload sshd 2>/dev/null || true
  fi
}

harden_firewall() {
  apt_ensure ufw
  info "Closing every door except the web and your own way in (port $SSH_PORT)."
  # What this does and does not cover: Docker publishes a container port by
  # writing its own firewall rules, which ufw never sees. That is fine here —
  # the only published ports are 80 and 443, which are open on purpose, and the
  # database is not published at all. Anything published later is open whatever
  # this says, so nothing else in the stack may publish a port.
  # No reset: a re-run must not drop a rule the operator added by hand. `allow`
  # is idempotent, so the three doors below are simply re-stated each time.
  run ufw default deny incoming >/dev/null
  run ufw default allow outgoing >/dev/null
  run ufw allow "$SSH_PORT/tcp" >/dev/null
  run ufw allow 80/tcp >/dev/null
  run ufw allow 443/tcp >/dev/null
  run ufw --force enable >/dev/null
}

harden_jail() {
  apt_ensure fail2ban
  # Ubuntu 24.04 has no /var/log/auth.log, so the systemd journal is the only
  # place the attempts are readable.
  write_file /etc/fail2ban/jail.d/otw.local 0644 <<JAIL
# Written by server-setup.sh — bans an address that keeps guessing.
[DEFAULT]
backend  = systemd
bantime  = 1h
findtime = 10m
maxretry = 5

[sshd]
enabled = true
port    = $SSH_PORT
JAIL
  run systemctl enable fail2ban >/dev/null 2>&1 || true
  run systemctl restart fail2ban >/dev/null 2>&1 || warn "The guess-blocker did not start; the firewall is still on."
}

harden_updates() {
  apt_ensure unattended-upgrades
  write_file /etc/apt/apt.conf.d/20auto-upgrades 0644 <<AUTOUP
// Written by server-setup.sh — security fixes install themselves.
APT::Periodic::Update-Package-Lists "1";
APT::Periodic::Unattended-Upgrade "1";
APT::Periodic::AutocleanInterval "7";
AUTOUP
  run systemctl enable --now unattended-upgrades >/dev/null 2>&1 || true
  info "Security fixes will install themselves from now on."
}

harden_swap() {
  local size="$SWAP_SIZE" mem_mb
  mem_mb=$(( $(awk '/^MemTotal:/ {print $2}' /proc/meminfo) / 1024 ))
  if [[ "$size" == "none" ]]; then info "Swap file: skipped on request."; return 0; fi
  if [[ -n "$SWAP_SIZE" && ! "$SWAP_SIZE" =~ ^[0-9]+G$ ]]; then
    die "\"$SWAP_SIZE\" is not a swap size I understand." "Write it as a whole number of gigabytes, like 2G, or as none."
  fi
  if [[ -z "$size" ]]; then
    (( mem_mb >= 3500 )) && { info "Enough memory — no swap file needed."; return 0; }
    size="2G"
  fi
  if [[ -n "$(swapon --show --noheadings 2>/dev/null)" ]]; then
    info "This machine already has swap space."
    return 0
  fi
  info "Adding a ${size} swap file so heavy work cannot kill the database."
  if [[ "$DRY_RUN" != "1" ]]; then
    # fallocate is instant but unusable on some filesystems (btrfs); dd always works.
    fallocate -l "$size" /swapfile 2>/dev/null \
      || dd if=/dev/zero of=/swapfile bs=1M count="$(( ${size%G} * 1024 ))" status=none
    chmod 600 /swapfile
    mkswap /swapfile >/dev/null
    swapon /swapfile
    grep -q '^/swapfile ' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
  fi
  write_file /etc/sysctl.d/99-otw.conf 0644 <<SYSCTL
# Written by server-setup.sh — prefer real memory, use the swap file only under pressure.
vm.swappiness = 10
vm.vfs_cache_pressure = 50
SYSCTL
  run sysctl --quiet --system >/dev/null 2>&1 || true
}

harden_docker() {
  if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    info "Docker is already installed."
  else
    info "Installing Docker (this is what runs OpenTraderWorld)."
    apt_ensure ca-certificates curl gnupg
    local vendor="$OS_ID"
    [[ "$vendor" == "ubuntu" || "$vendor" == "debian" ]] || vendor="debian"
    if [[ "$DRY_RUN" != "1" ]]; then
      install -m 0755 -d /etc/apt/keyrings
      curl -fsSL "https://download.docker.com/linux/$vendor/gpg" -o /etc/apt/keyrings/docker.asc \
        || die "Could not download Docker." "Run the same line again."
      chmod a+r /etc/apt/keyrings/docker.asc
      printf 'deb [arch=%s signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/%s %s stable\n' \
        "$(dpkg --print-architecture)" "$vendor" "$OS_CODENAME" > /etc/apt/sources.list.d/docker.list
      APT_UPDATED=0
    fi
    apt_ensure docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
    run systemctl enable --now docker >/dev/null 2>&1 || true
  fi

  # Unbounded container logs fill a small disk in a few months (plan.md §3).
  if [[ -e /etc/docker/daemon.json ]]; then
    grep -q 'max-size' /etc/docker/daemon.json 2>/dev/null \
      || warn "This machine has its own Docker settings file; leaving it alone. Check that its logs are capped."
  else
    write_file /etc/docker/daemon.json 0644 <<DOCKERD
{
  "log-driver": "json-file",
  "log-opts": { "max-size": "10m", "max-file": "3" },
  "live-restore": true
}
DOCKERD
    run systemctl restart docker >/dev/null 2>&1 || true
  fi

  # Membership in the docker group is administrator access to the machine by
  # another name; it is given to the install account only, never to anyone else.
  run usermod -aG docker "$OTW_USER"

  write_file /etc/systemd/journald.conf.d/otw.conf 0644 <<JOURNAL
# Written by server-setup.sh — the machine's own log never fills the disk.
[Journal]
SystemMaxUse=200M
JOURNAL
  run systemctl restart systemd-journald >/dev/null 2>&1 || true
}

phase_harden() {
  step "2/4  Protecting the machine"
  SSH_PORT="${SSH_PORT:-$(detect_ssh_port)}"
  save_conf

  if [[ -n "$TIMEZONE" ]]; then
    run timedatectl set-timezone "$TIMEZONE" || warn "Unknown timezone \"$TIMEZONE\" — leaving the machine's own."
  fi

  harden_account
  harden_keys
  harden_ssh
  harden_firewall
  harden_jail
  harden_updates
  harden_swap
  harden_docker
  info "The machine is protected."
}

# ─────────────────────────────────────────────────────────────────────────────
# install — the application itself, under the day-to-day account
# ─────────────────────────────────────────────────────────────────────────────

ADMIN_USER="admin"
ADMIN_PASS=""

gen_secret() {
  if command -v openssl >/dev/null 2>&1; then openssl rand -hex 12
  else head -c 12 /dev/urandom | od -An -tx1 | tr -d ' \n'; fi
}

fetch_deploy() {
  # A configured install is never overwritten: extracting over it would clobber
  # .env, network.env and dns.env, which is the whole database and the domain.
  if [[ -f "$DIR/deploy/.env" ]]; then
    info "OpenTraderWorld is already configured in $DIR — keeping it as it is."
    return 1
  fi
  info "Downloading OpenTraderWorld ($REPO@$REF)…"
  [[ "$DRY_RUN" == "1" ]] && { info "would download and unpack into $DIR/deploy"; return 1; }
  apt_ensure curl tar
  local tmp src
  tmp="$(mktemp -d)"
  # shellcheck disable=SC2064
  trap "rm -rf '$tmp'" RETURN
  curl -fsSL "https://codeload.github.com/$REPO/tar.gz/$REF" | tar -xz -C "$tmp" \
    || die "The download of OpenTraderWorld failed." "Run the same line again."
  src="$(find "$tmp" -mindepth 1 -maxdepth 1 -type d | head -n 1 || true)"
  [[ -f "$src/deploy/setup.sh" ]] || die \
    "The downloaded copy of OpenTraderWorld is not the one expected ($REPO@$REF)." \
    "Report this at https://github.com/$REPO/issues — nothing was installed."
  rm -rf "$DIR/deploy"
  install -d -o "$OTW_USER" -g "$OTW_USER" -m 755 "$DIR"
  mv "$src/deploy" "$DIR/deploy"
  chown -R "$OTW_USER:$OTW_USER" "$DIR"
  return 0
}

phase_install() {
  step "3/4  Installing OpenTraderWorld"
  ADMIN_PASS="$(gen_secret)"

  if ! fetch_deploy; then
    # Already configured, or a dry run: nothing to hand to setup.sh.
    [[ "$DRY_RUN" == "1" ]] && return 0
    ADMIN_PASS=""
    return 0
  fi

  info "Starting it up. The first run downloads the application — a few minutes."
  # The first-login password reaches setup.sh through a file only the install
  # account can read, never on a command line: anything on a command line is
  # readable by every account on the machine for as long as the process lives.
  local passfile="$DIR/.otw-bootstrap"
  printf 'OTW_ADMIN_PASSWORD=%s\n' "$ADMIN_PASS" > "$passfile"
  chmod 600 "$passfile"; chown "$OTW_USER:$OTW_USER" "$passfile"
  # setup.sh is interactive by design. OTW_NONINTERACTIVE makes every question
  # take the value already in the environment, so the operator is asked nothing
  # and the answers live here, in one place, next to the phase that knows them.
  #
  # `su - ` and not `runuser`: a login shell picks up the docker group added a
  # moment ago, which the current session does not have.
  local cmd rc=0
  cmd="$(printf 'cd %q && set -a && . ./.otw-bootstrap && set +a && OTW_NONINTERACTIVE=1 NET_CHOICE=4 OTW_DOMAIN=%q OTW_LOG=info ADMIN_USER=%q OVERWRITE=y CLEAN_VOLS=y WIPE_VOL=y START_NOW=Y bash deploy/setup.sh' \
    "$DIR" "$DOMAIN" "$ADMIN_USER")"
  su - "$OTW_USER" -c "$cmd" || rc=$?
  rm -f "$passfile"
  (( rc == 0 )) || die \
    "OpenTraderWorld could not be started." \
    "Run the same line again. If it stops here twice, send us what it printed:" \
    "    https://github.com/$REPO/issues"
}

# ─────────────────────────────────────────────────────────────────────────────
# cert — the address has to work from outside before the run is called a success
# ─────────────────────────────────────────────────────────────────────────────
phase_cert() {
  step "4/4  Making the address safe"
  [[ "$DRY_RUN" == "1" ]] && { info "would wait for https://$DOMAIN to answer"; return 0; }
  info "Waiting for the certificate for $DOMAIN. This takes up to a minute."
  local i code
  for i in $(seq 1 60); do
    code="$(curl -fsS -o /dev/null -w '%{http_code}' --max-time 5 "https://$DOMAIN/api/health" 2>/dev/null || true)"
    [[ "$code" == "200" ]] && { info "https://$DOMAIN answers. The padlock is real."; return 0; }
    sleep 5
  done
  # Hairpin NAT: at some providers the machine cannot reach its own public
  # address, so ask Caddy directly under the right name before declaring failure.
  code="$(curl -fsS -o /dev/null -w '%{http_code}' --max-time 5 --resolve "$DOMAIN:443:127.0.0.1" "https://$DOMAIN/api/health" 2>/dev/null || true)"
  if [[ "$code" == "200" ]]; then
    info "The certificate is in place and the application answers."
    warn "This machine cannot reach its own public address from the inside, which is normal at some providers."
    warn "Open https://$DOMAIN from your own computer to confirm."
    return 0
  fi
  die "The address https://$DOMAIN is not answering yet." \
      "The two usual reasons:" \
      "  · the name was pointed at this machine only minutes ago — wait ten minutes;" \
      "  · your provider has its own firewall in front of the machine, and ports 80 and 443" \
      "    are still closed there. Open them in your provider's control panel." \
      "Then run the same line again."
}

# ─────────────────────────────────────────────────────────────────────────────
# backup — the otw command, and a nightly copy that is taken and checked here
#
# The destination is this machine's own disk, deliberately. It is not a defence
# against the machine being deleted, and the card says so; it is a defence
# against the failures that actually happen — a bad update, a wrong click, a
# restore gone sideways — and it needs no account, no token and no provider.
# Measured on a real instance: a nightly copy is about 7 MB and the weekly full
# one about 41 MB, so a fortnight of history costs under 400 MB. Sending them
# off the machine is the next step and the card names it.
# ─────────────────────────────────────────────────────────────────────────────
phase_backup() {
  step "5/5  Backups, and the otw command"

  # The command lives next to the stack it drives, so a re-run of the installer
  # refreshes it along with everything else.
  if [[ -f "$DIR/deploy/otw" ]]; then
    run install -m 755 "$DIR/deploy/otw" /usr/local/bin/otw
  else
    warn "This version has no otw command to install; skipping it."
  fi

  write_file /etc/otw.conf 0644 <<OTWCONF
# Written by server-setup.sh — where the otw command finds the installation.
OTW_DIR=$DIR
OTW_USER=$OTW_USER
OTW_BACKUP_DIR=/var/backups/otw
OTWCONF

  write_file /etc/systemd/system/otw-backup.service 0644 <<UNIT
[Unit]
Description=OpenTraderWorld nightly backup
After=docker.service
Requires=docker.service

[Service]
Type=oneshot
ExecStart=/usr/local/bin/otw backup --scheduled
UNIT

  # A fixed minute across every install would have thousands of machines calling
  # their database at the same second; the spread is per-machine and stable.
  write_file /etc/systemd/system/otw-backup.timer 0644 <<TIMER
[Unit]
Description=OpenTraderWorld nightly backup

[Timer]
OnCalendar=*-*-* 03:20:00
RandomizedDelaySec=1800
Persistent=true

[Install]
WantedBy=timers.target
TIMER

  run install -d -m 700 /var/backups/otw
  run systemctl daemon-reload
  run systemctl enable --now otw-backup.timer >/dev/null 2>&1 || \
    warn "The nightly backup could not be scheduled. Take one by hand with: otw backup"

  # A backup nobody has ever taken is a plan, not a backup. This one runs now,
  # is checked, and its failure is loud rather than discovered in three months.
  if [[ "$DRY_RUN" != "1" ]]; then
    info "Taking the first backup now, and checking it can be read back…"
    if /usr/local/bin/otw backup >/dev/null 2>&1; then
      BACKUP_OK=1
      info "Done. From now on, one every night at about 03:20."
    else
      warn "The first backup did not work. Everything else is in place."
      warn "Run 'otw backup' once you are signed in, and 'otw report' if it fails again."
    fi
  fi
}

# ─────────────────────────────────────────────────────────────────────────────
# The card at the end (plan.md §4.2)
# ─────────────────────────────────────────────────────────────────────────────
final_card() {
  local card_file="$DIR/ACCOUNT.txt" pass_line access_line backup_line
  if [[ "$BACKUP_OK" == "1" ]]; then
    backup_line="every night on this machine, first one taken and checked"
  else
    backup_line="not running yet — type: otw backup"
  fi
  if [[ -n "$ADMIN_PASS" ]]; then
    pass_line="Password   $ADMIN_PASS     (save this now, it is not shown again)"
  else
    pass_line="Password   unchanged — this machine was already installed"
  fi
  if [[ -n "$ACCOUNT_PASS" ]]; then
    access_line="Machine    sign in as $OTW_USER with the password $ACCOUNT_PASS"
  elif [[ "$LOGIN_MODE" == "key" ]]; then
    access_line="Machine    sign in as $OTW_USER with your key — passwords are now refused"
  else
    access_line="Machine    sign in as $OTW_USER with your usual password"
  fi

  local card
  card="$(cat <<CARD

  Your OpenTraderWorld is ready.

  Address    https://$DOMAIN
  Username   $ADMIN_USER
  $pass_line
  $access_line
  Backup     $backup_line
  Updates    security fixes install themselves; for the app, type: otw update

CARD
)"
  if [[ "$DRY_RUN" != "1" && -n "$ADMIN_PASS" ]]; then
    printf '%s\n' "$card" > "$card_file"
    chmod 600 "$card_file"; chown "$OTW_USER:$OTW_USER" "$card_file"
  fi
  printf '\033[1m%s\033[0m\n' "$card"
  info "The same text is saved on this machine, in $card_file."
  info "To see it again later, sign in to this machine and type:  otw card"
  info "To see whether everything is well, type:  otw status"
  echo
  info "Copy the password into a password manager now. Your backup is on this"
  info "machine, which does not protect you if the machine itself disappears:"
  info "download a copy from Settings once in a while, or copy $BACKUP_DIR_PUBLIC elsewhere."
  echo
}

# ─────────────────────────────────────────────────────────────────────────────
# Run
# ─────────────────────────────────────────────────────────────────────────────
echo
bold "Here is what is about to happen on this machine"
info "1. It checks the machine, and that $DOMAIN really leads here."
info "2. It makes your day-to-day account, closes every door except the web,"
info "   and turns on automatic security fixes."
if [[ "$LOGIN_MODE" == "key" ]]; then
  info "   From then on this machine opens to your key only."
else
  info "   From then on you sign in as \"$OTW_USER\", and never as root."
fi
info "3. It installs OpenTraderWorld."
info "4. It gets the padlock for https://$DOMAIN and prints your password."
info "5. It sets up a backup every night, and takes the first one straight away."
echo
info "It takes about fifteen minutes, and asks you nothing more."
info "If anything stops it, run the same line again: it carries on where it stopped."
echo
[[ "$DRY_RUN" == "1" ]] && info "Dry run: nothing on this machine will be changed."
confirm "Go ahead?" y || { echo; info "Nothing was changed. Run the same line whenever you are ready."; echo; exit 0; }

bold "Putting $DOMAIN online"
save_conf

HAVE_KEYS=0
ACCOUNT_PASS=""
BACKUP_OK=0
BACKUP_DIR_PUBLIC="/var/backups/otw"
for phase in "${ALL_PHASES[@]}"; do
  wanted "$phase" || continue
  if phase_done "$phase" && [[ "$phase" != "check" ]]; then
    info "Already done: $phase — skipping."
    continue
  fi
  "phase_$phase"
  mark_done "$phase"
done

final_card
