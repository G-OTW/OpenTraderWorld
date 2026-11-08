-- Chart alerts gain two states judged at the bar's close: `close_above` and `close_below`
-- fire on every closed bar that ends past the other series, where `above`, `below` and
-- `crosses` fire only on the bar that crosses it.
ALTER TABLE histviz_alerts DROP CONSTRAINT IF EXISTS histviz_alerts_op_check;
ALTER TABLE histviz_alerts
    ADD CONSTRAINT histviz_alerts_op_check
    CHECK (op IN ('above', 'below', 'crosses', 'close_above', 'close_below'));
