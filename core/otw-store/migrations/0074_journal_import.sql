-- Trading Journal — generic trade-book import.
--
-- Importing someone else's trade book is a mapping problem: every broker names its
-- columns differently, in a different language, with a different number/date locale.
-- The pipeline is: detect → the user validates → the mapping is saved → the next file
-- from the same source maps itself. Nothing is written until the user validates, and
-- every write is tagged with a batch so a bad import can be reverted whole.
--
-- Naming: an *import mapping* (this table) maps source columns onto trade fields. It is
-- NOT a journal *template* (journal_templates), which is the form used to log a trade by
-- hand. The two are unrelated objects with unrelated shapes.

-- ── Import mappings ──────────────────────────────────────────────────────────
-- `mapping` is the whole document needed to replay an import deterministically:
--   { shape, header_row, delimiter, decimal, date_order, tz_offset,
--     columns: { "<col index>": "<target field>" }, value_maps, conventions, defaults }
-- `fingerprint` is the sha256 of the normalized+sorted header set: a re-uploaded file
-- from the same source matches its mapping without asking. `headers` keeps the same
-- normalized list so a near-miss file can still be scored (Jaccard) and suggested.
CREATE TABLE IF NOT EXISTS journal_import_mappings (
    id           UUID PRIMARY KEY,
    name         TEXT NOT NULL DEFAULT 'Untitled mapping',
    fingerprint  TEXT NOT NULL DEFAULT '',
    headers      JSONB NOT NULL DEFAULT '[]'::jsonb,
    mapping      JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_journal_import_mappings_fp
    ON journal_import_mappings(fingerprint);

-- ── Import batches ───────────────────────────────────────────────────────────
-- One row per committed import. Trades point back at it, so reverting an import is
-- "delete the trades of this batch" — the rest of the journal is untouched.
CREATE TABLE IF NOT EXISTS journal_import_batches (
    id           UUID PRIMARY KEY,
    mapping_id   UUID REFERENCES journal_import_mappings(id) ON DELETE SET NULL,
    mapping_name TEXT NOT NULL DEFAULT '',
    filename     TEXT NOT NULL DEFAULT '',
    category_id  UUID REFERENCES journal_categories(id) ON DELETE SET NULL,
    shape        TEXT NOT NULL DEFAULT 'roundtrip',
    source_rows  INTEGER NOT NULL DEFAULT 0,
    imported     INTEGER NOT NULL DEFAULT 0,
    duplicates   INTEGER NOT NULL DEFAULT 0,
    failed       INTEGER NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_journal_import_batches_at
    ON journal_import_batches(created_at DESC);

-- Trades carry their batch + a hash of the source row(s) they came from. The hash makes
-- re-importing the same file a no-op (unique index) while still allowing two genuinely
-- identical trades in one file (the hash includes the occurrence index).
ALTER TABLE journal_trades
    ADD COLUMN IF NOT EXISTS import_batch_id UUID
        REFERENCES journal_import_batches(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS import_row_hash TEXT;
CREATE INDEX IF NOT EXISTS idx_journal_trades_batch ON journal_trades(import_batch_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_journal_trades_row_hash
    ON journal_trades(import_row_hash) WHERE import_row_hash IS NOT NULL;

-- ── Learned header aliases ───────────────────────────────────────────────────
-- Every correction the user makes in the mapping step teaches the detector: the
-- normalized source header is remembered against the field it was mapped to, and the
-- next file that uses that header maps itself. Shipped synonyms live in code; this
-- table is the user's own vocabulary and wins over them.
CREATE TABLE IF NOT EXISTS journal_import_aliases (
    header     TEXT PRIMARY KEY,
    field      TEXT NOT NULL,
    hits       INTEGER NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
