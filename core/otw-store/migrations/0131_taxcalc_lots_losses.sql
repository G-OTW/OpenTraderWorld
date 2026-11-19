-- TaxCalculator: cost-basis method per profile, and the loss carry-forward registry.
--
-- 1. **Cost method.** Which acquisition a sale is matched against is a rule of the tax
--    residence, not a preference: France uses the weighted average price (PMP), the US and
--    Germany first-in-first-out, the UK the same-day / 30-day / section 104 pool sequence.
--    NULL = the regime template's method.
-- 2. **Loss registry.** A loss is a period fact: the net result of one tax year in one
--    netting pool. What is still carried into a later year (after the gains it absorbed and
--    the years it expired) is derived on read, never stored, so correcting an old year
--    corrects every year after it.

ALTER TABLE taxcalc_profiles ADD COLUMN IF NOT EXISTS cost_method TEXT;
ALTER TABLE taxcalc_profiles DROP CONSTRAINT IF EXISTS taxcalc_profiles_cost_method_check;
ALTER TABLE taxcalc_profiles ADD CONSTRAINT taxcalc_profiles_cost_method_check
    CHECK (cost_method IS NULL OR cost_method IN ('average', 'fifo', 'uk_pool'));

CREATE TABLE IF NOT EXISTS taxcalc_loss_years (
    profile_id  UUID NOT NULL REFERENCES taxcalc_profiles(id) ON DELETE CASCADE,
    tax_year    INTEGER NOT NULL,
    -- Netting pool key, as the regime template names it (e.g. 'securities', 'crypto').
    pool        TEXT NOT NULL,
    -- The year's own net realized result in that pool, before any carried loss, in the
    -- profile currency. Negative = a loss that year.
    net         DOUBLE PRECISION NOT NULL,
    note        TEXT NOT NULL DEFAULT '',
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (profile_id, tax_year, pool)
);
