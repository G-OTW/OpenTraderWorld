-- Watchlists module.
--
-- Named lists of instruments the user wants to keep an eye on (no positions, no ledger —
-- that's the Portfolio Tracker). Each item pins an exact provider symbol (CoinGecko coin id
-- for crypto, Yahoo ticker for stocks/ETFs) exactly like portfolio_assets, so pricing is never
-- ambiguous. The latest computed quote (USD spot, 24h/3d/7d/30d changes, 30-day sparkline) is
-- cached per item as JSONB — history is re-derived from a rolling provider fetch, never stored
-- as rows. Per-list auto-sync with a user-chosen refresh interval; a background loop refreshes
-- due lists. Single-user: no owner scoping.

-- ── Watchlists ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS watchlists (
    id             UUID PRIMARY KEY,
    name           TEXT NOT NULL DEFAULT 'New watchlist',
    description    TEXT NOT NULL DEFAULT '',
    -- When TRUE the background loop refreshes this list every `refresh_secs`.
    sync_enabled   BOOLEAN NOT NULL DEFAULT FALSE,
    -- Auto-refresh cadence, seconds. Floor of 60 keeps a misbehaving client from
    -- hammering the free providers; 900 (15 min) is the smooth default.
    refresh_secs   INTEGER NOT NULL DEFAULT 900 CHECK (refresh_secs >= 60),
    position       DOUBLE PRECISION NOT NULL DEFAULT 0,
    -- Last successful quote refresh (manual or auto), NULL until first refresh.
    refreshed_at   TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_watchlists_pos ON watchlists(position);

-- ── Items ────────────────────────────────────────────────────────────────────
-- One row per watched instrument. `quote` is the cached last computation:
--   { price_usd, change_24h, change_3d, change_7d, change_30d,
--     spark: [daily closes oldest→newest], history: [[unix_sec, close]…], history_at }
-- `history` is the rolling ~30-day daily series the changes/spark derive from; it is
-- refetched at most every few hours while spot updates every refresh.
CREATE TABLE IF NOT EXISTS watchlist_items (
    id             UUID PRIMARY KEY,
    watchlist_id   UUID NOT NULL REFERENCES watchlists(id) ON DELETE CASCADE,
    asset_class    TEXT NOT NULL DEFAULT 'crypto'
                   CHECK (asset_class IN ('crypto', 'stock', 'etf')),
    provider       TEXT NOT NULL DEFAULT 'coingecko'
                   CHECK (provider IN ('coingecko', 'yahoo')),
    provider_id    TEXT NOT NULL,
    symbol         TEXT NOT NULL DEFAULT '',
    name           TEXT NOT NULL DEFAULT '',
    -- Exchange label reported by the provider (e.g. "NasdaqGS"); display-only.
    exchange       TEXT NOT NULL DEFAULT '',
    notes          TEXT NOT NULL DEFAULT '',
    position       DOUBLE PRECISION NOT NULL DEFAULT 0,
    quote          JSONB NOT NULL DEFAULT '{}'::jsonb,
    quoted_at      TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (watchlist_id, provider, provider_id)
);
CREATE INDEX IF NOT EXISTS idx_watchlist_items_wl ON watchlist_items(watchlist_id);
