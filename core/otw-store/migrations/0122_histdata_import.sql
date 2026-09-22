-- Importing an OHLCV series from a file the user already has.
--
-- An imported series is a dataset like any other, filed under the reserved provider id
-- `import` so no code path ever hands it to a connector. What a download reads from its
-- provider, an import has to be told: the broker/venue the file came from, and a name the
-- user recognizes it by. Those three columns carry that.

ALTER TABLE histdata_datasets
    ADD COLUMN IF NOT EXISTS label  TEXT,                            -- the user's own name for the series
    ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT '',        -- where the file came from (free text)
    ADD COLUMN IF NOT EXISTS tags   JSONB NOT NULL DEFAULT '[]'::jsonb;

-- Two datasets of the same instrument from two brokers are two different series, and the
-- user imported both on purpose, so `source` is part of the identity of an imported one.
-- It is not part of a downloaded one's: there the provider already says where it came from.
-- One partial unique index each, so re-importing the same file still lands on one dataset.
ALTER TABLE histdata_datasets
    DROP CONSTRAINT IF EXISTS histdata_datasets_provider_asset_type_ticker_timeframe_key;

CREATE UNIQUE INDEX IF NOT EXISTS uq_histdata_datasets_coords
    ON histdata_datasets (provider, asset_type, ticker, timeframe)
    WHERE provider <> 'import';

CREATE UNIQUE INDEX IF NOT EXISTS uq_histdata_datasets_import
    ON histdata_datasets (asset_type, ticker, timeframe, source)
    WHERE provider = 'import';
