-- The market a currency is priced on, stated by the user rather than guessed.
--
-- `fx_histdata` tries USD<ccy>, <ccy>USD and the two stablecoin pairs, which finds a major
-- and little else: a venue names its pair its own way (Binance lists USDTUSD, Coinbase
-- USDC-USD), and the guess takes whichever granted connector happens to serve the asset
-- type. That is the one place the journal was still guessing an instrument.
--
-- A stated source answers it once per currency: one connector, one ticker, one direction.
-- When it is set it is the only route tried, for reading a stored series and for queueing
-- the download that creates one. `invert` says which way the ticker is quoted, because the
-- ticker alone does not (USDJPY quotes JPY per USD, USDTUSD quotes USD per USDT).
CREATE TABLE IF NOT EXISTS journal_fx_sources (
    quote        TEXT PRIMARY KEY,
    -- Kept nullable so deleting a connector leaves the mapping visible and repairable
    -- instead of dropping it silently; the API refuses to use a row without one.
    connector_id UUID REFERENCES histdata_connectors(id) ON DELETE SET NULL,
    provider     TEXT NOT NULL,
    asset_type   TEXT NOT NULL,
    ticker       TEXT NOT NULL,
    invert       BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
