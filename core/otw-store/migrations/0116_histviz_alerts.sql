-- Chart alerts: a level on an instrument, watched server-side.
--
-- The point of the feature is that it works with no browser open, so an alert is a row here
-- and a job in `otw-core`, never a timer in the page. It is evaluated on **closed bars** of
-- the instrument's own timeframe: an alert that fired on a wick the exchange later revised
-- would be a lie, and the closed bar is the same thing the chart and the backtester read.
--
-- Two kinds share one row on purpose:
--   * 'price'     compares a price series (close/high/low) with `value`;
--   * 'indicator' compares a custom-indicator definition from the shared library
--     (`backtest_indicators`, the same DAG the chart draws) with `value`.
-- One row, one comparison, so the vocabulary the user learns is `op` and nothing else.
CREATE TABLE IF NOT EXISTS histviz_alerts (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT        NOT NULL DEFAULT '',
    -- The instrument, in the coordinates every other chart route speaks.
    provider      TEXT        NOT NULL,
    asset_type    TEXT        NOT NULL,
    ticker        TEXT        NOT NULL,
    timeframe     TEXT        NOT NULL,
    -- Which account to read through. NULL = the oldest connector the chart is granted for
    -- that provider, resolved at evaluation time so a deleted connector does not orphan it.
    connector_id  UUID        REFERENCES histdata_connectors(id) ON DELETE SET NULL,
    kind          TEXT        NOT NULL CHECK (kind IN ('price', 'indicator')),
    -- Which series the level is compared against: 'close' | 'high' | 'low' for a price
    -- alert, ignored for an indicator one (the definition is the series).
    source        TEXT        NOT NULL DEFAULT 'close',
    indicator_id  UUID        REFERENCES backtest_indicators(id) ON DELETE CASCADE,
    op            TEXT        NOT NULL CHECK (op IN ('above', 'below', 'crosses')),
    value         DOUBLE PRECISION NOT NULL,
    -- Channels to push to; [] = every channel the chart is granted. Same shape as the
    -- watchlist alerts, so the notification broker sees one kind of caller.
    channels      JSONB       NOT NULL DEFAULT '[]'::jsonb,
    -- Fire again after the first hit, or disable itself. A level crossed once is usually
    -- crossed several times in the same hour, which is why the cooldown exists beside it.
    repeat        BOOLEAN     NOT NULL DEFAULT false,
    cooldown_secs INT         NOT NULL DEFAULT 0,
    enabled       BOOLEAN     NOT NULL DEFAULT true,
    -- Evaluation bookkeeping. `last_bar_ts` is what makes the job idempotent: a bar is
    -- judged once, however often the loop wakes up.
    last_bar_ts   TIMESTAMPTZ,
    last_value    DOUBLE PRECISION,
    last_fired_at TIMESTAMPTZ,
    fire_count    INT         NOT NULL DEFAULT 0,
    checked_at    TIMESTAMPTZ,
    -- Why the last evaluation could not run (no connector, unknown symbol, quota). Kept on
    -- the row so the alert list can say it instead of going quiet.
    last_error    TEXT        NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- An indicator alert without a definition has nothing to measure.
    CONSTRAINT histviz_alerts_indicator CHECK (kind <> 'indicator' OR indicator_id IS NOT NULL)
);

CREATE INDEX IF NOT EXISTS histviz_alerts_due_idx ON histviz_alerts (enabled, checked_at);
CREATE INDEX IF NOT EXISTS histviz_alerts_instrument_idx
    ON histviz_alerts (provider, asset_type, ticker, timeframe);
