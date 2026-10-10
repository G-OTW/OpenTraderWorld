-- Lower-timeframe candles a backtest or a paper session downloaded for the bars where the order
-- of a stop and a target mattered. A cache, never a dataset: it holds scattered windows, so it
-- stays out of the catalog, and a window is fetched once.
CREATE TABLE IF NOT EXISTS histdata_intrabar_windows (
    provider   TEXT NOT NULL,
    asset_type TEXT NOT NULL,
    ticker     TEXT NOT NULL,
    timeframe  TEXT NOT NULL,
    from_ts    TIMESTAMPTZ NOT NULL,
    to_ts      TIMESTAMPTZ NOT NULL,
    bars       INT NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, asset_type, ticker, timeframe, from_ts)
);

CREATE TABLE IF NOT EXISTS histdata_intrabar_bars (
    provider   TEXT NOT NULL,
    asset_type TEXT NOT NULL,
    ticker     TEXT NOT NULL,
    timeframe  TEXT NOT NULL,
    ts         TIMESTAMPTZ NOT NULL,
    open       NUMERIC NOT NULL,
    high       NUMERIC NOT NULL,
    low        NUMERIC NOT NULL,
    close      NUMERIC NOT NULL,
    bid_open   NUMERIC,
    bid_high   NUMERIC,
    bid_low    NUMERIC,
    bid_close  NUMERIC,
    ask_open   NUMERIC,
    ask_high   NUMERIC,
    ask_low    NUMERIC,
    ask_close  NUMERIC,
    PRIMARY KEY (provider, asset_type, ticker, timeframe, ts)
);
