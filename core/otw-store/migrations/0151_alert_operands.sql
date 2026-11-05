-- Chart alerts compare two series, an expiry, and script inputs.
--
-- An alert used to be one series (`kind` + `source` | `indicator_id`) against one level
-- (`value`). It is now two operands in the backtest's own vocabulary (`Operand`: price,
-- const, indicator, custom_indicator, metric), so "SMA 9 crosses above SMA 21", "close
-- crosses an EMA", "change over 5 bars above 3 %" and "a script plot crosses 0" are all the
-- same row, evaluated by the same engine a backtest runs:
--   * lhs     the series watched;
--   * rhs     what it is compared with (a const for a plain level);
--   * inputs  input values of the scripts either side reads, keyed by indicator id. Empty =
--             each script's defaults;
--   * expires_at  NULL = never. Past it the alert is no longer watched and is disabled.
ALTER TABLE histviz_alerts
    ADD COLUMN IF NOT EXISTS lhs        JSONB,
    ADD COLUMN IF NOT EXISTS rhs        JSONB,
    ADD COLUMN IF NOT EXISTS inputs     JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;

UPDATE histviz_alerts
   SET lhs = CASE
           WHEN kind = 'indicator'
               THEN jsonb_build_object('kind', 'custom_indicator', 'id', indicator_id::text)
           ELSE jsonb_build_object('kind', 'price', 'field', source)
       END,
       rhs = jsonb_build_object('kind', 'const', 'value', value)
 WHERE lhs IS NULL;

ALTER TABLE histviz_alerts
    ALTER COLUMN lhs SET NOT NULL,
    ALTER COLUMN rhs SET NOT NULL,
    DROP CONSTRAINT IF EXISTS histviz_alerts_indicator,
    DROP COLUMN IF EXISTS kind,
    DROP COLUMN IF EXISTS source,
    DROP COLUMN IF EXISTS indicator_id,
    DROP COLUMN IF EXISTS value;
