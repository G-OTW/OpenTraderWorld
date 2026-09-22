# Backup & restore

Everything lives in one PostgreSQL database, so a backup is one `pg_dump`. **Settings → Backup & restore** in the app shows these commands pre-filled for your deployment. Run them on the host where the stack is deployed; they use the existing Postgres container, no extra access needed.

The section has two tabs, each split into **Backup** and **Restore**:

- **Full**: the whole database, taken on the host, for when the machine dies.
- **Partial**: the modules you tick, as one zip, for moving house or keeping a readable copy.

## Full backup

Plain dump:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld > otw-backup-$(date +%F).sql
```

### Encrypt it (recommended)

A dump contains your data in the clear. Pipe it through `gpg` (or `age`) so the file is encrypted on disk. You'll be prompted for a passphrase:

```bash
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld | gpg -c --cipher-algo AES256 -o otw-backup-$(date +%F).sql.gpg
```

## Security notes

- **API keys and provider credentials** (news feeds, market-data providers) are already encrypted at rest with `OTW_SECRET_KEY`, so they appear only as ciphertext in the dump.
- Back up **`OTW_SECRET_KEY`** (from `deploy/.env`) **separately**, not inside the same dump, or those encrypted secrets can't be restored.
- The dump includes live **session tokens**. Treat the file as a secret, or drop the `sessions` table after restoring and sign in again.
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
