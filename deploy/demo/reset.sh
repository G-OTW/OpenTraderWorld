#!/usr/bin/env bash
# Demo sandbox reset — restore the database from the `otw_seed` template.
#
#   ./reset.sh --init    build otw_seed (migrate + --seed-demo), then do a first reset
#   ./reset.sh           reset (cron: */15 * * * *, quarter-hour aligned to match the
#                        countdown the app shows from GET /api/demo)
#
# The reset is template-based (DROP + CREATE ... TEMPLATE otw_seed): sub-second and
# atomic. Uploaded files are demo-blocked, so the uploads volume stays empty; core is
# restarted anyway to drop any in-memory state.

set -euo pipefail
cd "$(dirname "$0")"

# .env in this directory is auto-loaded by compose (interpolation + secrets) — no
# --env-file needed, so manual `docker compose …` commands can't silently miss it.
COMPOSE=(docker compose -f docker-compose.demo.yml)
PSQL=("${COMPOSE[@]}" exec -T postgres psql -U otw -v ON_ERROR_STOP=1)

# Preflight: prove the .env password actually opens the database OVER THE NETWORK.
# (Loopback/exec tests lie — the postgres image trusts local connections.) The classic
# failure is an edited .env against a volume initialized with the old password:
# postgres only applies POSTGRES_PASSWORD on first volume init.
PW=$(grep '^POSTGRES_PASSWORD=' .env | cut -d= -f2-)
if ! docker run --rm --network "$(docker inspect otw-demo-postgres-1 --format '{{range $k,$v := .NetworkSettings.Networks}}{{$k}}{{end}}')" \
    -e PGPASSWORD="$PW" postgres:18-alpine \
    psql -h postgres -U otw -d postgres -tAc 'SELECT 1' >/dev/null 2>&1; then
  echo "ERROR: the POSTGRES_PASSWORD in .env does not open the database." >&2
  echo "Most likely the postgres volume was initialized with an older password." >&2
  echo "Demo data is throwaway — recreate it:" >&2
  echo "  docker compose -f docker-compose.demo.yml down && docker volume rm otw-demo_postgres-data && docker compose -f docker-compose.demo.yml up -d && ./reset.sh --init" >&2
  exit 1
fi

if [[ "${1:-}" == "--init" ]]; then
  echo "building otw_seed template…"
  "${PSQL[@]}" -d postgres -c "DROP DATABASE IF EXISTS otw_seed WITH (FORCE);"
  "${PSQL[@]}" -d postgres -c "CREATE DATABASE otw_seed OWNER otw;"
  # Migrate + load fixtures into the seed DB (one-shot container, no server started).
  "${COMPOSE[@]}" run --rm \
    -e DATABASE_URL="postgres://otw:$(grep '^POSTGRES_PASSWORD=' .env | cut -d= -f2-)@postgres/otw_seed" \
    core --seed-demo
  echo "otw_seed ready."
fi

# Sanity: refuse to reset if the template is missing (a bad cron entry must not leave
# the demo without a database).
if ! "${PSQL[@]}" -d postgres -tAc "SELECT 1 FROM pg_database WHERE datname='otw_seed'" | grep -q 1; then
  echo "otw_seed template missing — run ./reset.sh --init first" >&2
  exit 1
fi

# Stop core so no pool connection blocks the drop, then restore and restart.
"${COMPOSE[@]}" stop core >/dev/null
"${PSQL[@]}" -d postgres -c "DROP DATABASE IF EXISTS opentraderworld WITH (FORCE);"
"${PSQL[@]}" -d postgres -c "CREATE DATABASE opentraderworld OWNER otw TEMPLATE otw_seed;"
"${COMPOSE[@]}" start core >/dev/null
echo "demo reset done ($(date -u +%FT%TZ))"
