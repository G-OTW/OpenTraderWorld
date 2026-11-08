-- Portfolio Tracker: drift, concentration and digest notifications.
--
-- Channels are ids from notif_channels; an empty list means every channel granted to the
-- portfolios module. Each alert fires once per breach: the `alerted` state is what stops a
-- second message while the breach lasts and re-arms it once the book is back in line.

ALTER TABLE portfolios
    ADD COLUMN IF NOT EXISTS drift_notify          BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS alert_channels        UUID[] NOT NULL DEFAULT '{}',
    -- Largest share of net worth one holding may take, in percent. NULL = no limit.
    ADD COLUMN IF NOT EXISTS max_weight_pct        DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS concentration_notify  BOOLEAN NOT NULL DEFAULT FALSE,
    -- Assets already reported above the limit, so a breach is told once.
    ADD COLUMN IF NOT EXISTS concentration_alerted UUID[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS digest                TEXT NOT NULL DEFAULT 'off',
    ADD COLUMN IF NOT EXISTS digest_channels       UUID[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS digest_last_at        TIMESTAMPTZ,
    -- Prices (portfolio currency) of each asset at the last digest: the movers of the next
    -- one are measured against them, and the first digest has none rather than invented ones.
    ADD COLUMN IF NOT EXISTS digest_marks          JSONB NOT NULL DEFAULT '{}';

ALTER TABLE portfolios DROP CONSTRAINT IF EXISTS portfolios_digest_check;
ALTER TABLE portfolios ADD CONSTRAINT portfolios_digest_check
    CHECK (digest IN ('off', 'daily', 'weekly'));

ALTER TABLE portfolios DROP CONSTRAINT IF EXISTS portfolios_max_weight_check;
ALTER TABLE portfolios ADD CONSTRAINT portfolios_max_weight_check
    CHECK (max_weight_pct IS NULL OR (max_weight_pct > 0 AND max_weight_pct <= 100));

-- A bucket already reported out of band. Kept per bucket because a target set is rewritten
-- wholesale on every save, and the save carries this flag over for the buckets it keeps.
ALTER TABLE portfolio_targets
    ADD COLUMN IF NOT EXISTS alerted          BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS last_notified_at TIMESTAMPTZ;
