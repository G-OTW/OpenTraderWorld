-- MyWealth: what an asset knows about itself beyond its revisions.
--
-- Each column is optional and only means something for the kinds that use it:
--   loan                 a liability's terms: { principal, rate_pct, term_months, start, payment }.
--                        Its balance follows the amortization formula, written as one revision
--                        per month by the module's job, so the curve needs no special case.
--   paired_liability_id  the debt held against this asset (a house and its mortgage), for
--                        equity and loan-to-value. Deleting the debt clears the link.
--   drift_pct_year       an assumed yearly rate (+3 for property, -15 for a car). Applied on
--                        read from the last real revision, never stored as one.
--   broker_account_id    a broker account whose equity is read live (kind broker_account).
--   chain, address       a public crypto address read on chain (kind wallet).
--   owner                the household member the asset belongs to, free text.
ALTER TABLE wealth_assets
    ADD COLUMN IF NOT EXISTS loan                JSONB,
    ADD COLUMN IF NOT EXISTS paired_liability_id UUID REFERENCES wealth_assets(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS drift_pct_year      DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS broker_account_id   UUID REFERENCES broker_accounts(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS chain               TEXT,
    ADD COLUMN IF NOT EXISTS address             TEXT,
    ADD COLUMN IF NOT EXISTS owner               TEXT;

ALTER TABLE wealth_assets DROP CONSTRAINT IF EXISTS wealth_assets_chain_check;
ALTER TABLE wealth_assets ADD CONSTRAINT wealth_assets_chain_check
    CHECK (chain IS NULL OR chain IN ('btc', 'eth'));

CREATE INDEX IF NOT EXISTS idx_wealth_assets_owner ON wealth_assets(owner);
