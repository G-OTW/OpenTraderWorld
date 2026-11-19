-- MyWealth: what a category is, and what share of the net worth it should be.
--
-- Categories stay free text on the asset; this table only holds the flags a category carries.
-- `liquid` marks money that can be spent within a month (cash, a brokerage account), which is
-- what the runway divides. A category with no row is illiquid: the cautious answer.
CREATE TABLE IF NOT EXISTS wealth_categories (
    name       TEXT PRIMARY KEY,
    liquid     BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Target share per category with a tolerance band, the same shape as portfolio_targets.
-- "" is the uncategorized bucket.
CREATE TABLE IF NOT EXISTS wealth_targets (
    category   TEXT PRIMARY KEY,
    target_pct DOUBLE PRECISION NOT NULL CHECK (target_pct >= 0 AND target_pct <= 100),
    band_pct   DOUBLE PRECISION NOT NULL DEFAULT 5 CHECK (band_pct >= 0)
);
