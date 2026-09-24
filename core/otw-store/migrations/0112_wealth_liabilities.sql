-- MyWealth: liabilities, and a live link to a portfolio.
--
-- Two changes that come from the same decision about where the boundary between the two
-- modules is. It is not by asset type, it is by **how a thing is valued**: anything with a
-- price series belongs to the Portfolio Tracker, and MyWealth is the balance sheet, which is
-- the illiquid, the owed, and *references* to the portfolios.
--
-- 1. **Liabilities.** A net worth without debt is not a net worth. A mortgage is an asset
--    with a negative sign and a remaining balance that is revised like any other value, so
--    it needs one column, not a second table with its own revisions and its own FX pass.
-- 2. **The live link.** Importing a portfolio copied its market value into a revision, and
--    that copy was wrong the next morning. An asset can now *point* at a portfolio and be
--    valued from it at read time, which is the only version of this feature that stays true.

ALTER TABLE wealth_assets
    -- +1 for something owned, -1 for something owed. A sign rather than a boolean because
    -- it is what the sum multiplies by, and a boolean would need a branch at every use.
    ADD COLUMN IF NOT EXISTS sign         SMALLINT NOT NULL DEFAULT 1,
    -- When set, this asset's value is the portfolio's net worth, read live. Its revisions
    -- are then history, not the source of truth.
    ADD COLUMN IF NOT EXISTS portfolio_id UUID REFERENCES portfolios(id) ON DELETE SET NULL,
    -- How often this value is expected to be refreshed, in days. A house valued three years
    -- ago is wrong in silence, and silence is what every tracker gets wrong here. 0 = never
    -- nag (a linked or a fixed-value asset).
    ADD COLUMN IF NOT EXISTS review_days  INTEGER NOT NULL DEFAULT 0;

ALTER TABLE wealth_assets DROP CONSTRAINT IF EXISTS wealth_assets_sign_check;
ALTER TABLE wealth_assets ADD CONSTRAINT wealth_assets_sign_check CHECK (sign IN (-1, 1));

CREATE INDEX IF NOT EXISTS idx_wealth_assets_pf ON wealth_assets(portfolio_id);

-- A liability is a kind of thing, not a kind of number: the type vocabulary grows with it so
-- a mortgage is not filed as "other".
ALTER TABLE wealth_assets DROP CONSTRAINT IF EXISTS wealth_assets_type_check;
ALTER TABLE wealth_templates DROP CONSTRAINT IF EXISTS wealth_templates_type_check;
