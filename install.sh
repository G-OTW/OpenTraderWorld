#!/usr/bin/env bash
#
# OpenTraderWorld — one-command installer.
#
#   curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
#
# Downloads only the deploy/ directory (compose stack + setup script) and runs the
# interactive setup, which pulls prebuilt images from Docker Hub — no Rust/Node
# toolchain needed. With --build, clones the full source instead and builds the
# images locally.
#
# Options (flags win over the matching env var):
#   --dir <path>   Install directory              (OTW_DIR,  default ./opentraderworld)
#   --ref <ref>    Branch or tag to install       (OTW_REF,  default master)
#   --build        Full source checkout + local image build (requires git)
#   -h, --help     Show this help
#
#   OTW_REPO       GitHub repo slug               (default G-OTW/OpenTraderWorld)
set -euo pipefail

REPO="${OTW_REPO:-G-OTW/OpenTraderWorld}"
REF="${OTW_REF:-master}"
DIR="${OTW_DIR:-$PWD/opentraderworld}"
BUILD=0
# Flags forwarded verbatim to deploy/setup.sh.
SETUP_ARGS=()

bold() { printf '\033[1m%s\033[0m\n' "$1"; }
info() { printf '  %s\n' "$1"; }
die()  { printf 'error: %s\n' "$1" >&2; exit 1; }

usage() {
  cat <<EOF
OpenTraderWorld — one-command installer.

  curl -fsSL https://raw.githubusercontent.com/${REPO}/master/install.sh | bash

Options (flags win over the matching env var):
  --dir <path>   Install directory              (OTW_DIR,  default ./opentraderworld)
  --ref <ref>    Branch or tag to install       (OTW_REF,  default master)
  --build        Full source checkout + local image build (requires git)
  -h, --help     Show this help

  OTW_REPO       GitHub repo slug               (default G-OTW/OpenTraderWorld)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dir)   [[ $# -ge 2 ]] || die "--dir needs a path"; DIR="$2"; shift 2 ;;
    --ref)   [[ $# -ge 2 ]] || die "--ref needs a branch or tag"; REF="$2"; shift 2 ;;
    --build) BUILD=1; shift ;;
    -y|--yes) SETUP_ARGS+=(--yes); shift ;;
    --wipe-volumes) SETUP_ARGS+=(--wipe-volumes); shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1 (see --help)" ;;
  esac
done

bold "OpenTraderWorld — installer"
echo

# ── Prerequisites ──
command -v docker >/dev/null 2>&1 \
  || die "Docker is required — https://docs.docker.com/get-docker/"
docker compose version >/dev/null 2>&1 \
  || die "Docker Compose v2 is required (the 'docker compose' plugin, bundled with Docker Desktop)"
docker info >/dev/null 2>&1 \
  || die "the Docker daemon is not running — start Docker and re-run"
if [[ "$BUILD" == "1" ]]; then
  command -v git >/dev/null 2>&1 || die "--build requires git (full source checkout)"
else
  command -v curl >/dev/null 2>&1 || die "curl is required"
  command -v tar  >/dev/null 2>&1 || die "tar is required"
fi

# ── Target directory ──
# Never touch an existing install: extracting over it would clobber network.env /
# dns.env (rewritten in-app by Settings → Network). Re-runs go through setup.sh.
# A configured install (.env written) is never touched — extracting over it would clobber
# network.env / dns.env. Deploy files without an .env are a partial/aborted install, which
# is safe to re-extract: that's the retry path after setup bailed out.
if [[ -f "$DIR/deploy/.env" ]]; then
  die "a configured install already exists in $DIR — re-run its own setup instead:
    cd $DIR/deploy && ./setup.sh"
fi
if [[ -e "$DIR/deploy/setup.sh" ]]; then
  info "Found deploy files from an earlier attempt (no .env) — refreshing them."
  rm -rf "$DIR/deploy"
elif [[ -d "$DIR" ]] && [[ -n "$(ls -A "$DIR" 2>/dev/null)" ]]; then
  die "$DIR exists and is not empty — pick another location with --dir"
fi

# ── Fetch ──
if [[ "$BUILD" == "1" ]]; then
  info "Cloning ${REPO}@${REF} (full source, --build)…"
  git clone --depth 1 --branch "$REF" "https://github.com/${REPO}.git" "$DIR"
else
  info "Fetching deploy files from ${REPO}@${REF}…"
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  curl -fsSL "https://codeload.github.com/${REPO}/tar.gz/${REF}" | tar -xz -C "$TMP" \
    || die "download failed — check the repo/ref (${REPO}@${REF}) and your connection"
  # The tarball unpacks into a single <repo>-<ref> top-level directory.
  SRC="$(find "$TMP" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
  [[ -f "$SRC/deploy/setup.sh" ]] || die "no deploy/setup.sh in ${REPO}@${REF} — wrong repo or ref?"
  mkdir -p "$DIR"
  mv "$SRC/deploy" "$DIR/deploy"
fi
info "Installed deploy files into $DIR"
echo

# ── Hand off to setup ──
# stdin is deliberately left alone: under `curl | bash` it is the pipe, and
# reattaching it process-wide here did not survive the exec into setup.sh.
# setup.sh's prompt_default reads from /dev/tty per prompt instead, which works
# whether or not stdin is a terminal. With no tty at all it stops and points at
# --yes rather than silently taking defaults.
if [[ ! -t 0 ]] && ! { : </dev/tty; } 2>/dev/null; then
  info "No terminal available — setup will stop at its first prompt; re-run with --yes to accept defaults."
fi

cd "$DIR"
if [[ "$BUILD" == "1" ]]; then
  exec bash deploy/setup.sh --build ${SETUP_ARGS+"${SETUP_ARGS[@]}"}
else
  exec bash deploy/setup.sh ${SETUP_ARGS+"${SETUP_ARGS[@]}"}
fi
