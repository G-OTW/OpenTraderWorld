-- Portfolio Tracker: target allocation.
--
-- One row per bucket of one dimension. `dimension` is `asset_class` today; `sector`,
-- `region` and `currency` are the same table tomorrow and need no schema change, which is
-- the point of keying it rather than adding four columns.
--
-- Cash is a bucket, never an asset row: it comes from the ledger walk, and inventing a
-- synthetic cash asset to hold it would pollute the positions table, the allocation donut
-- and every per-asset query in the module.

CREATE TABLE IF NOT EXISTS portfolio_targets (
    portfolio_id UUID NOT NULL REFERENCES portfolios(id) ON DELETE CASCADE,
    dimension    TEXT NOT NULL DEFAULT 'asset_class',
    bucket       TEXT NOT NULL,
    -- Share of net worth this bucket should hold, in percent.
    target_pct   DOUBLE PRECISION NOT NULL DEFAULT 0,
    -- How far it may drift before the row is out of band, in percentage points. A target
    -- with no tolerance is a target nobody can ever be at.
    band_pct     DOUBLE PRECISION NOT NULL DEFAULT 5,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (portfolio_id, dimension, bucket)
);

-- A tracker that only knows crypto, stocks and ETFs cannot hold a balanced allocation, and
-- the drift table it feeds would have three rows for a book with bonds in it.
ALTER TABLE portfolio_assets DROP CONSTRAINT IF EXISTS portfolio_assets_asset_class_check;
ALTER TABLE portfolio_assets ADD CONSTRAINT portfolio_assets_asset_class_check
    CHECK (asset_class IN ('crypto', 'stock', 'etf', 'bond', 'commodity', 'real_estate', 'alternative'));
