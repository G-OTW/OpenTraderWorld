-- The tick and size grid a trade on alert is materialized on, read from the broker when the
-- trade is armed, so a fire sends without asking the broker first. Holds the account and
-- instrument it was read for; NULL (or another instrument) reads it again at fire time.
ALTER TABLE alert_trades ADD COLUMN IF NOT EXISTS grid JSONB;

-- The time alert scheduler asks for the next instant and the due ones on every wake.
CREATE INDEX IF NOT EXISTS histviz_alerts_fire_at_idx
    ON histviz_alerts (fire_at) WHERE enabled AND op = 'at_time';
