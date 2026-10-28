# Backup & restore

Everything lives in one PostgreSQL database, so a backup is one `pg_dump`. **Settings → Backup & restore** in the app shows the same commands. Run them on the host where the stack is deployed, from the install directory (the one that holds `deploy/`); they use the existing Postgres container, no extra access needed.

The section has two tabs, each split into **Backup** and **Restore**:

- **Full**: the whole database, taken on the host, for when the machine dies.
- **Partial**: the modules you tick, as one zip, for moving house or keeping a readable copy.

## Full backup

Plain dump:

```bash
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner > otw-backup-$(date +%F).sql
```

### Encrypt it (recommended)

A dump contains your data in the clear. Pipe it through `gpg` (or `age`) so the file is encrypted on disk. You'll be prompted for a passphrase; `GPG_TTY` and `--pinentry-mode loopback` let that prompt reach your terminal while the dump fills gpg's input:

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner \
  | gpg --symmetric --cipher-algo AES256 --pinentry-mode loopback \
  > otw-backup-$(date +%F).sql.gpg
```

### Automatic backup {#automatic-backup}

When a backup script on the host reports to the app (the [server installer](#server-installs) sets one up), the **Full** tab opens with an **Automatic backup** card: how long ago the last backup ran, where the copy went, whether it was checked, the schedule and the size. It turns to a warning when the last run failed, or after 48 hours without one. Without such a script there is no card, and the commands above are the whole story.

## Security notes

- **API keys and provider credentials** (news feeds, market-data providers) are already encrypted at rest with `OTW_SECRET_KEY`, so they appear only as ciphertext in the dump.
- Back up **`OTW_SECRET_KEY`** (from `deploy/.env`) **separately**, not inside the same dump, or those encrypted secrets can't be restored.
- The dump includes live **session tokens**. Treat the file as a secret, or empty the `sessions` table after restoring (`DELETE FROM sessions;`) and sign in again. Don't drop the table: sign-in needs it.
- Store the encrypted backup **off-box** and rotate older copies.

## Partial (per module)

The **Partial** tab takes the modules you tick and gives you **one zip file**, to move a journal to another instance or keep a readable copy. It runs with the app up, unlike the full backup.

### Partial backup

1. Open **Settings → Backup & restore → Partial → Backup**.
2. Tick the modules you want. Each one shows its row count and size, the ticked set is totalled under the list, and *What is in the selection* breaks that down table by table. Historical bars start unticked: they are the largest table by far, and they can be downloaded again from your provider.
3. Leave **Include stored provider credentials** off unless you know why you need it. Those values are encrypted with this instance's `OTW_SECRET_KEY` and are unreadable anywhere else.
4. Click **Download selected data**. You get `otw-data-YYYY-MM-DD.zip`.

Inside the zip: `manifest.json` (what it holds, which version wrote it) and one `tables/<name>.jsonl` per table, one JSON object per row. Any tool can read it.

### Partial restore

1. Open **Settings → Backup & restore → Partial → Restore** on the target instance and pick the file.
2. The file is read as soon as you pick it, and nothing is written: you get the version that wrote it and, per module and per table, how many rows it carries against how many are here now. A very large table is reported as an estimate, marked `~`. A file this instance would refuse (damaged, or from a newer release) is refused at this point, before you commit to anything.
3. Choose how it should meet the data already there:
   - **Add what is missing** keeps everything present and adds only rows that are not there yet. Nothing is overwritten.
   - **Replace** erases the data of every module in the file, then loads the file's version. It asks you to type `REPLACE`, and it saves a copy of the current data first.

   The line under the choice turns those counts into what will happen. Replace is exact: *erases the N rows here, puts the M rows from the file in their place*. Merge can only give a ceiling, *adds up to M rows*: a row whose key is already here is skipped, and only the load itself knows how many that is.
4. Click **Load this file**.

Everything happens in one transaction: if any part fails, nothing is changed.

::: warning A file from a newer version is refused
Loading a bundle written by a newer release is refused rather than attempted. Update the instance first, then load it.
:::

Rows keep their original identifiers, and the whole file is read in memory, so a very large selection (historical bars, typically) is refused with a message pointing back at the `pg_dump` backup above. That is the right tool for "everything, including what I never look at".

## Full restore

A dump restores into an empty database only. Core runs its migrations on every boot, so a database it has already started on (any running stack, including one `setup.sh` just created) collides with the dump. Stop core, replace the database with an empty one, load the dump, start core again:

```bash
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q < otw-backup-YYYY-MM-DD.sql
docker compose -f deploy/docker-compose.yml start core
```

The `DROP DATABASE` line erases what the stack holds now: take a dump of it first if it matters. `ON_ERROR_STOP=1` stops at the first error instead of loading half a dump.

From an encrypted backup:

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
gpg -d otw-backup-YYYY-MM-DD.sql.gpg \
  | docker compose -f deploy/docker-compose.yml exec -T postgres \
    psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q
docker compose -f deploy/docker-compose.yml start core
```

Make sure the restored stack uses the **same `OTW_SECRET_KEY`** as when the backup was taken, or stored provider credentials will be unreadable.

## Server installs {#server-installs}

A machine set up with the [server installer](/guide/hosting/when-it-breaks#installer-options) has the `otw` command, which does all of the above for you:

| Command | What it does |
|---|---|
| `otw backup` | Takes a backup now into `/var/backups/otw` and checks that the file is complete. |
| `otw restore` | Lists the backups on the machine, asks which one, and puts it back once you type `RESTORE`. It takes a full backup of the current data first, then restarts core. |

The installer also schedules `otw backup` every night at about 03:20, which is what lights up the [Automatic backup](#automatic-backup) card. A nightly copy leaves out the two tables that can be downloaded again (the instrument catalog and the historical bars); Sunday's copy is full. The last 10 nightly and 2 full copies are kept. They stay on the machine: copy them somewhere else to survive a dead server.
