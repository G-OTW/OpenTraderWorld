-- Portfolio Tracker module.
--
-- User-owned portfolios that track the live value of crypto / stocks / ETFs. Each portfolio holds
-- assets; each asset is built from an operations ledger (buy/sell) from which current quantity and
-- average cost are derived. Prices are fetched and stored in USD (CoinGecko for crypto, Yahoo for
-- stocks/etf), then converted to the portfolio's display currency at read time using the journal's
-- USD-based fx_rates (carry-forward). A daily job (per-portfolio opt-in) plus manual refresh writes
-- one valuation snapshot per portfolio per day, building a value time series. Single-user: no owner
-- scoping. Distinct from manager_portfolios (the Dataroma scrape cache).

-- ── Portfolios ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS portfolios (
    id             UUID PRIMARY KEY,
    name           TEXT NOT NULL DEFAULT 'New portfolio',
    description    TEXT NOT NULL DEFAULT '',
    -- Currency the portfolio's values are displayed in (USD prices converted at read time).
    currency       TEXT NOT NULL DEFAULT 'USD',
    -- When TRUE the daily job snapshots & refreshes this portfolio once per day.
    auto_refresh   BOOLEAN NOT NULL DEFAULT FALSE,
    position       DOUBLE PRECISION NOT NULL DEFAULT 0,
    -- Last successful price refresh (manual or auto), NULL until first refresh.
    refreshed_at   TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_portfolios_pos ON portfolios(position);

-- ── Assets ───────────────────────────────────────────────────────────────────
-- One row per instrument held in a portfolio. `provider`+`provider_id` pin the exact symbol so
-- pricing is never ambiguous (BTC vs BTC/USD vs BTC-USD): for crypto provider='coingecko' and
-- provider_id is the coin id ("bitcoin"); for stocks/etf provider='yahoo' and provider_id is the
-- Yahoo ticker ("AAPL"). `symbol`/`name` are display labels. `last_price_usd` is the most recent
-- spot in USD; `last_price_at` when it was fetched.
CREATE TABLE IF NOT EXISTS portfolio_assets (
    id              UUID PRIMARY KEY,
    portfolio_id    UUID NOT NULL REFERENCES portfolios(id) ON DELETE CASCADE,
    asset_class     TEXT NOT NULL DEFAULT 'crypto'
                    CHECK (asset_class IN ('crypto', 'stock', 'etf')),
    provider        TEXT NOT NULL DEFAULT 'coingecko'
                    CHECK (provider IN ('coingecko', 'yahoo')),
    provider_id     TEXT NOT NULL,
    symbol          TEXT NOT NULL DEFAULT '',
    name            TEXT NOT NULL DEFAULT '',
    last_price_usd  DOUBLE PRECISION,
    last_price_at   TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (portfolio_id, provider, provider_id)
);
CREATE INDEX IF NOT EXISTS idx_portfolio_assets_pf ON portfolio_assets(portfolio_id);

-- ── Operations (ledger) ──────────────────────────────────────────────────────
-- Buy/sell of an asset. Current quantity = Σ(buy qty) − Σ(sell qty); average cost & realized PnL
-- derive from this ledger. `price` and `fee` are in the asset's native price currency (USD, since
-- prices are USD); `op_date` is the trade date. Free-form `note`.
CREATE TABLE IF NOT EXISTS portfolio_operations (
    id           UUID PRIMARY KEY,
    asset_id     UUID NOT NULL REFERENCES portfolio_assets(id) ON DELETE CASCADE,
    side         TEXT NOT NULL DEFAULT 'buy' CHECK (side IN ('buy', 'sell')),
    op_date      DATE NOT NULL DEFAULT CURRENT_DATE,
    quantity     DOUBLE PRECISION NOT NULL DEFAULT 0,
    price        DOUBLE PRECISION NOT NULL DEFAULT 0,
    fee          DOUBLE PRECISION NOT NULL DEFAULT 0,
    note         TEXT NOT NULL DEFAULT '',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_portfolio_ops_asset ON portfolio_operations(asset_id, op_date);

-- ── Valuation snapshots ──────────────────────────────────────────────────────
-- One row per portfolio per day: the total market value (in the portfolio's display currency at
-- snapshot time) and total cost basis. Powers the value-over-time chart. A manual refresh or the
-- daily job upserts the row for today.
CREATE TABLE IF NOT EXISTS portfolio_snapshots (
    portfolio_id  UUID NOT NULL REFERENCES portfolios(id) ON DELETE CASCADE,
    snap_date     DATE NOT NULL DEFAULT CURRENT_DATE,
    currency      TEXT NOT NULL DEFAULT 'USD',
    market_value  DOUBLE PRECISION NOT NULL DEFAULT 0,
    cost_basis    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (portfolio_id, snap_date)
);

-- ── CoinGecko coin list cache ────────────────────────────────────────────────
-- /coins/list is requestable at most every ~30 min; we cache it so asset search resolves symbol →
-- coin id offline. Single global cache (id=TRUE). `coins` is the raw [{id,symbol,name}] array.
CREATE TABLE IF NOT EXISTS portfolio_coingecko_cache (
    id          BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    coins       JSONB NOT NULL DEFAULT '[]'::jsonb,
    fetched_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
