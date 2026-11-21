-- MyWealth settings: goals, the stale-valuation notification and the CPI series.
--
--   goal_value, goal_date   target net worth in the display currency, and by when.
--   monthly_expenses        what a month costs, in the display currency. Feeds the FIRE
--                           number, the savings rate and the runway.
--   stale_notify            'off' | 'weekly'. Aged valuations pushed through a channel.
--   stale_channels          channel ids to narrow to; empty = every channel granted to wealth.
--   stale_last_at           when the weekly pass last ran, so it runs once a week.
--   cpi_series_id           the Fundamentals series used to deflate the curve; null = the
--                           US CPI (FRED CPIAUCSL) when it is loaded.
ALTER TABLE wealth_settings
    ADD COLUMN IF NOT EXISTS goal_value       DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS goal_date        DATE,
    ADD COLUMN IF NOT EXISTS monthly_expenses DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS stale_notify     TEXT NOT NULL DEFAULT 'off',
    ADD COLUMN IF NOT EXISTS stale_channels   JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS stale_last_at    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS cpi_series_id    UUID;

ALTER TABLE wealth_settings DROP CONSTRAINT IF EXISTS wealth_settings_stale_notify_check;
ALTER TABLE wealth_settings ADD CONSTRAINT wealth_settings_stale_notify_check
    CHECK (stale_notify IN ('off', 'weekly'));
