-- Portfolio Tracker: what the user knows about a holding that no quote carries.
--
-- Dividend data, sector and country are user entered. Nothing here is filled by a guess:
-- an empty field is shown as "no estimate" or "unclassified", never as a zero.

ALTER TABLE portfolio_assets
    -- Amount paid per share per payment, in the asset's currency.
    ADD COLUMN IF NOT EXISTS div_per_share DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS div_frequency TEXT NOT NULL DEFAULT 'none',
    ADD COLUMN IF NOT EXISTS div_next_ex   DATE,
    ADD COLUMN IF NOT EXISTS div_next_pay  DATE,
    -- Why the line is held, and when to look at that reason again.
    ADD COLUMN IF NOT EXISTS thesis        TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS review_at     DATE,
    ADD COLUMN IF NOT EXISTS sector        TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS country       TEXT NOT NULL DEFAULT '';

ALTER TABLE portfolio_assets DROP CONSTRAINT IF EXISTS portfolio_assets_div_frequency_check;
ALTER TABLE portfolio_assets ADD CONSTRAINT portfolio_assets_div_frequency_check
    CHECK (div_frequency IN ('none', 'monthly', 'quarterly', 'semi', 'annual'));

ALTER TABLE portfolio_assets DROP CONSTRAINT IF EXISTS portfolio_assets_div_per_share_check;
ALTER TABLE portfolio_assets ADD CONSTRAINT portfolio_assets_div_per_share_check
    CHECK (div_per_share IS NULL OR div_per_share >= 0);

CREATE INDEX IF NOT EXISTS idx_portfolio_assets_review ON portfolio_assets(review_at)
    WHERE review_at IS NOT NULL;
