-- Portfolio Tracker: planned trades and a goal per portfolio.

-- A buy or a sell the user intends at a price. Checked against the asset's own quote each
-- time it is priced; `hit_at` is set once, when the quote reaches the level.
CREATE TABLE IF NOT EXISTS portfolio_plans (
    id           UUID PRIMARY KEY,
    portfolio_id UUID NOT NULL REFERENCES portfolios(id) ON DELETE CASCADE,
    asset_id     UUID NOT NULL REFERENCES portfolio_assets(id) ON DELETE CASCADE,
    side         TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    -- Level in the asset's own currency: a buy waits for the quote at or below it, a sell
    -- for the quote at or above it.
    price        DOUBLE PRECISION NOT NULL CHECK (price > 0),
    qty          DOUBLE PRECISION CHECK (qty IS NULL OR qty > 0),
    note         TEXT NOT NULL DEFAULT '',
    notify       BOOLEAN NOT NULL DEFAULT TRUE,
    hit_at       TIMESTAMPTZ,
    hit_price    DOUBLE PRECISION,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_portfolio_plans_pf ON portfolio_plans(portfolio_id);
CREATE INDEX IF NOT EXISTS idx_portfolio_plans_waiting ON portfolio_plans(asset_id) WHERE hit_at IS NULL;

CREATE TABLE IF NOT EXISTS portfolio_goals (
    portfolio_id         UUID PRIMARY KEY REFERENCES portfolios(id) ON DELETE CASCADE,
    target_value         DOUBLE PRECISION NOT NULL CHECK (target_value > 0),
    target_date          DATE NOT NULL,
    -- What the user plans to add each month, in the portfolio currency.
    monthly_contribution DOUBLE PRECISION NOT NULL DEFAULT 0 CHECK (monthly_contribution >= 0),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
