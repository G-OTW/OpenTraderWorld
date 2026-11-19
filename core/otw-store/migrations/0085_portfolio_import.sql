-- Portfolio Tracker — operations-ledger import.
--
-- Same pipeline as the journal's trade-book import (detect → the user validates → the
-- mapping is saved → the next file from the same source maps itself), but one row is one
-- **operation** (a buy or a sell against one asset), not a trade. What the file cannot
-- state — which asset of *this* portfolio a symbol is — is asked, never guessed.

-- ── Learned header aliases: shared, and scoped per target set ────────────────
-- The same header means different things to different modules: "Date" is a trade's entry
-- to the journal and an operation's date to a portfolio. One table, one scope column, so
-- a vocabulary taught in one place cannot mis-map a file in another.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables
               WHERE table_schema = current_schema() AND table_name = 'journal_import_aliases') THEN
        ALTER TABLE journal_import_aliases RENAME TO import_aliases;
    END IF;
END $$;

ALTER TABLE import_aliases ADD COLUMN IF NOT EXISTS scope TEXT NOT NULL DEFAULT 'journal';
ALTER TABLE import_aliases DROP CONSTRAINT IF EXISTS journal_import_aliases_pkey;
ALTER TABLE import_aliases DROP CONSTRAINT IF EXISTS import_aliases_pkey;
ALTER TABLE import_aliases ADD CONSTRAINT import_aliases_pkey PRIMARY KEY (scope, header);

-- ── Saved mappings ───────────────────────────────────────────────────────────
-- A source-column → operation-field document, matched to a new file by header
-- fingerprint. Not scoped to a portfolio: the same broker export feeds any of them.
CREATE TABLE IF NOT EXISTS portfolio_import_mappings (
    id           UUID PRIMARY KEY,
    name         TEXT NOT NULL DEFAULT 'Untitled mapping',
    fingerprint  TEXT NOT NULL DEFAULT '',
    headers      JSONB NOT NULL DEFAULT '[]'::jsonb,
    mapping      JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_portfolio_import_mappings_fp
    ON portfolio_import_mappings(fingerprint);

-- ── Batches ──────────────────────────────────────────────────────────────────
-- One row per committed import, against the portfolio it landed in. Operations point
-- back at it, so reverting an import is "delete the operations of this batch".
CREATE TABLE IF NOT EXISTS portfolio_import_batches (
    id             UUID PRIMARY KEY,
    portfolio_id   UUID REFERENCES portfolios(id) ON DELETE CASCADE,
    mapping_id     UUID REFERENCES portfolio_import_mappings(id) ON DELETE SET NULL,
    mapping_name   TEXT NOT NULL DEFAULT '',
    filename       TEXT NOT NULL DEFAULT '',
    source_rows    INTEGER NOT NULL DEFAULT 0,
    imported       INTEGER NOT NULL DEFAULT 0,
    duplicates     INTEGER NOT NULL DEFAULT 0,
    failed         INTEGER NOT NULL DEFAULT 0,
    assets_created INTEGER NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_portfolio_import_batches_at
    ON portfolio_import_batches(created_at DESC);

-- ── Operations carry where they came from ────────────────────────────────────
-- `portfolio_id` is denormalized from the asset (an asset never moves between
-- portfolios, so it is immutable) because de-duplication is **per portfolio** — a
-- portfolio is the book being kept, the same statement may legitimately feed two of
-- them, and an index cannot reach through a join to say so.
ALTER TABLE portfolio_operations
    ADD COLUMN IF NOT EXISTS portfolio_id UUID REFERENCES portfolios(id) ON DELETE CASCADE,
    ADD COLUMN IF NOT EXISTS import_batch_id UUID
        REFERENCES portfolio_import_batches(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS import_row_hash TEXT;

UPDATE portfolio_operations o
SET portfolio_id = a.portfolio_id
FROM portfolio_assets a
WHERE a.id = o.asset_id AND o.portfolio_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_portfolio_ops_batch ON portfolio_operations(import_batch_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_portfolio_ops_row_hash
    ON portfolio_operations(portfolio_id, import_row_hash) WHERE import_row_hash IS NOT NULL;
