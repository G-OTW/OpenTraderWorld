-- Chart alerts gain a time alert: `at_time` fires once when the clock reaches `fire_at`, on
-- the alert's instrument, with no series compared. `lhs` and `rhs` stay filled (close against
-- 0) so every reader of the row keeps a valid operand pair.
ALTER TABLE histviz_alerts ADD COLUMN IF NOT EXISTS fire_at TIMESTAMPTZ;
ALTER TABLE histviz_alerts DROP CONSTRAINT IF EXISTS histviz_alerts_op_check;
ALTER TABLE histviz_alerts
    ADD CONSTRAINT histviz_alerts_op_check
    CHECK (op IN ('above', 'below', 'crosses', 'close_above', 'close_below', 'at_time'));
ALTER TABLE histviz_alerts DROP CONSTRAINT IF EXISTS histviz_alerts_fire_at_check;
ALTER TABLE histviz_alerts
    ADD CONSTRAINT histviz_alerts_fire_at_check
    CHECK (op <> 'at_time' OR fire_at IS NOT NULL);
