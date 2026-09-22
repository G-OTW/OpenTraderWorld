-- Managers' Portfolios module.
--
-- Mirrors the public superinvestor 13F summaries published by Dataroma. We scrape the
-- managers summary list plus each manager's holdings detail on a schedule and cache them here,
-- so the UI reads from Postgres (never hits Dataroma per request). Attribution to Dataroma is
-- shown in the UI. Distinct from the future user "portfolios" module. Single-user: no owner scoping.

CREATE TABLE IF NOT EXISTS manager_portfolios (
    id           UUID PRIMARY KEY,
    -- Dataroma manager code (the ?m= slug, e.g. "BRK", "psc"). Stable identifier upstream.
    slug         TEXT NOT NULL UNIQUE,
    -- "Warren Buffett - Berkshire Hathaway" (full label as shown).
    name         TEXT NOT NULL,
    -- Portfolio value as displayed ("$263.1 B") and a numeric form (USD) for sorting.
    value_text   TEXT NOT NULL DEFAULT '',
    value_num    DOUBLE PRECISION,
    stock_count  INTEGER NOT NULL DEFAULT 0,
    -- Reporting period label, e.g. "Q1 2026".
    period       TEXT NOT NULL DEFAULT '',
    -- Canonical Dataroma detail URL for this manager.
    source_url   TEXT NOT NULL DEFAULT '',
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS manager_holdings (
    id              UUID PRIMARY KEY,
    portfolio_id    UUID NOT NULL REFERENCES manager_portfolios(id) ON DELETE CASCADE,
    -- Row order within the portfolio (as listed upstream, by weight).
    position        INTEGER NOT NULL DEFAULT 0,
    ticker          TEXT NOT NULL DEFAULT '',
    company         TEXT NOT NULL DEFAULT '',
    -- Percent of the portfolio (0..100).
    pct             DOUBLE PRECISION,
    -- Recent activity text ("Buy", "Add 5%", "Reduce 12%", "Sell", "") as published.
    activity        TEXT NOT NULL DEFAULT '',
    shares          DOUBLE PRECISION,
    reported_price  DOUBLE PRECISION,
    value           DOUBLE PRECISION,
    current_price   DOUBLE PRECISION,
    -- Percent change of current vs reported price.
    change_pct      DOUBLE PRECISION,
    week52_low      DOUBLE PRECISION,
    week52_high     DOUBLE PRECISION
);

CREATE INDEX IF NOT EXISTS idx_manager_holdings_portfolio ON manager_holdings(portfolio_id);
-- Ticker filter (which portfolios hold X) reads this.
CREATE INDEX IF NOT EXISTS idx_manager_holdings_ticker ON manager_holdings(ticker);
