-- FinanceDatabase module ("findb").
--
-- A searchable catalog of ~300k financial instruments imported from
-- JerBouma/FinanceDatabase, plus a per-user favorites system organized in folders.
-- Single-user: no owner scoping.
--
-- The instrument catalog is bulk-loaded once on module install (see otw-core findb_import).
-- `findb_meta` records the installed data version so install is idempotent.

-- One row per instrument across all asset classes. Category-specific columns are nullable.
CREATE TABLE IF NOT EXISTS findb_instruments (
    id          BIGSERIAL PRIMARY KEY,
    asset_type  TEXT NOT NULL,            -- equity | etf | fund | index | moneymarket | currency | crypto
    symbol      TEXT NOT NULL,
    name        TEXT NOT NULL DEFAULT '',
    currency    TEXT NOT NULL DEFAULT '',
    exchange    TEXT NOT NULL DEFAULT '',
    summary     TEXT NOT NULL DEFAULT '',
    -- Equities
    sector      TEXT NOT NULL DEFAULT '',
    industry    TEXT NOT NULL DEFAULT '',
    country     TEXT NOT NULL DEFAULT '',
    market_cap  TEXT NOT NULL DEFAULT '',
    isin        TEXT NOT NULL DEFAULT '',
    -- Funds/ETFs/indices
    category    TEXT NOT NULL DEFAULT '',
    family      TEXT NOT NULL DEFAULT ''
);

-- Trigram indexes power fuzzy "search engine" matching on symbol + name.
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX IF NOT EXISTS idx_findb_symbol_trgm ON findb_instruments USING gin (symbol gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_findb_name_trgm   ON findb_instruments USING gin (name   gin_trgm_ops);
-- Facet filters.
CREATE INDEX IF NOT EXISTS idx_findb_asset_type  ON findb_instruments(asset_type);

-- Install/version marker. version = '' means not installed yet.
CREATE TABLE IF NOT EXISTS findb_meta (
    id          BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    version     TEXT NOT NULL DEFAULT '',
    count       BIGINT NOT NULL DEFAULT 0,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO findb_meta (id, version)
SELECT TRUE, ''
WHERE NOT EXISTS (SELECT 1 FROM findb_meta);

-- User folders for organizing favorites (flat list of named categories).
CREATE TABLE IF NOT EXISTS findb_folders (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL DEFAULT '',
    color       TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Saved instruments. An instrument may be favorited once; folder is optional (NULL = unfiled).
CREATE TABLE IF NOT EXISTS findb_favorites (
    id            UUID PRIMARY KEY,
    instrument_id BIGINT NOT NULL REFERENCES findb_instruments(id) ON DELETE CASCADE,
    folder_id     UUID REFERENCES findb_folders(id) ON DELETE SET NULL,
    note          TEXT NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (instrument_id)
);
CREATE INDEX IF NOT EXISTS idx_findb_fav_folder ON findb_favorites(folder_id);
