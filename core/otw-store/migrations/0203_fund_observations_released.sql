-- When an observation was published, when the source says so. A value is known from its
-- release, not from the start of the period it describes: a chart or a backtest that places
-- January's CPI on January 1st reads it a month and a half early. NULL = the source gives no
-- release time, and readers fall back to the period start.
ALTER TABLE fund_observations
    ADD COLUMN IF NOT EXISTS released_at TIMESTAMPTZ;
