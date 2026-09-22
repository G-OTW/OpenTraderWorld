-- Portfolio Tracker: the daily curve, and what it takes to rebuild it.
--
-- `portfolio_snapshots` held a market value and a cost basis, which is enough to draw a line
-- and not enough to measure anything: every deposit reads as a rally. A return series needs
-- the external cash that moved on the day, so the curve can be deposit-adjusted (TWR) before
-- a volatility, a drawdown or a Sharpe is computed on it.
--
-- It also needs to exist. Snapshots start the day the user opts in, so a portfolio created
-- today has no history at all. `source` separates a snapshot taken from a live price from one
-- rebuilt out of the ledger and stored daily bars, so neither ever overwrites the other.

ALTER TABLE portfolio_snapshots
    -- Uninvested cash on the day, display currency. `market_value` stays positions-only, so
    -- the existing chart is unchanged; net worth is the sum of the two.
    ADD COLUMN IF NOT EXISTS cash             DOUBLE PRECISION NOT NULL DEFAULT 0,
    -- Net external cash of the day (deposits minus withdrawals), signed. The TWR term.
    ADD COLUMN IF NOT EXISTS flow             DOUBLE PRECISION NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS income           DOUBLE PRECISION NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS fees             DOUBLE PRECISION NOT NULL DEFAULT 0,
    -- The same value in USD. A portfolio's display currency can be changed at any time, and
    -- without this the stored history would silently become a mix of two currencies.
    ADD COLUMN IF NOT EXISTS market_value_usd DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS source           TEXT NOT NULL DEFAULT 'live';

ALTER TABLE portfolio_snapshots DROP CONSTRAINT IF EXISTS portfolio_snapshots_source_check;
ALTER TABLE portfolio_snapshots ADD CONSTRAINT portfolio_snapshots_source_check
    CHECK (source IN ('live', 'rebuilt'));

-- A CoinGecko coin id is a *spot* coordinate, not a bar ticker, and a Yahoo ticker only
-- happens to be both. The bar symbol is asked once and never guessed: an asset without one
-- is named in the coverage report rather than approximated to something that prices.
ALTER TABLE portfolio_assets
    ADD COLUMN IF NOT EXISTS hist_symbol   TEXT NOT NULL DEFAULT '',
    -- Currency the bars are quoted in. A spot is fetched in USD by construction, a stored
    -- bar is quoted in whatever the listing trades in, and marking a Paris line's history
    -- with a dollar sign would be wrong by the exchange rate every single day.
    ADD COLUMN IF NOT EXISTS hist_currency TEXT NOT NULL DEFAULT 'USD';

ALTER TABLE portfolios
    -- What the portfolio is measured against: {"asset_type":"equity","symbol":"SPY"}.
    -- NULL = no benchmark, and the benchmark block is absent rather than zero-filled.
    ADD COLUMN IF NOT EXISTS benchmark    JSONB,
    -- Oldest day the stored curve is known to be wrong from: a backdated operation
    -- invalidates every snapshot after it, and rebuilding the whole history each time would
    -- re-download years of bars to fix one week.
    ADD COLUMN IF NOT EXISTS rebuild_from DATE,
    -- Risk-free rate used by Sharpe and Sortino, as an annual fraction (0.03 = 3%).
    ADD COLUMN IF NOT EXISTS risk_free    DOUBLE PRECISION NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_portfolio_snapshots_date ON portfolio_snapshots(portfolio_id, snap_date);
