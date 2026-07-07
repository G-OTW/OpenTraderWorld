# Backup & restore

Everything lives in one PostgreSQL database, so a backup is one `pg_dump`. **Settings → Backup** in the app shows these commands pre-filled for your deployment — run them on the host where the stack is deployed; they use the existing Postgres container, no extra access needed.

## Take a backup

Plain dump:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld > otw-backup-$(date +%F).sql
```

### Encrypt it (recommended)

A dump contains your data in the clear. Pipe it through `gpg` (or `age`) so the file is encrypted on disk — you'll be prompted for a passphrase:

```bash
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld | gpg -c --cipher-algo AES256 -o otw-backup-$(date +%F).sql.gpg
```

## Security notes

- **API keys and provider credentials** (news feeds, market-data providers) are already encrypted at rest with `OTW_SECRET_KEY`, so they appear only as ciphertext in the dump.
- Back up **`OTW_SECRET_KEY`** (from `deploy/.env`) **separately** — not inside the same dump — or those encrypted secrets can't be restored.
- The dump includes live **session tokens**. Treat the file as a secret, or drop the `sessions` table after restoring and sign in again.
- Store the encrypted backup **off-box** and rotate older copies.

## Restore

Into a fresh, empty database (a newly created stack):

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld < otw-backup-2026-07-06.sql
```

From an encrypted backup:

```bash
gpg -d otw-backup-2026-07-06.sql.gpg | \
  docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld
```

Make sure the restored stack uses the **same `OTW_SECRET_KEY`** as when the backup was taken, or stored provider credentials will be unreadable.
