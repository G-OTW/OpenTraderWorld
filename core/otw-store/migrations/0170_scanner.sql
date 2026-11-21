-- Scanner: a universe of items (an instrument on a data connector at a timeframe), grouped
-- into hives, scanned by alerts that AND reusable signals on each closed bar.
--
-- The scanner only notifies. A hit links to the chart, where chart alerts and trades live.
-- Single-user: no owner scoping.

-- One instrument at one timeframe. `connector_id` NULL = the oldest connector granted to the
-- scanner for that provider, resolved at evaluation time so a deleted connector does not
-- orphan an item (hence no foreign key). The cache columns are written by the evaluator on
-- each closed bar it reads, so the board costs nothing beyond the scans.
CREATE TABLE IF NOT EXISTS scanner_items (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider      TEXT        NOT NULL,
    asset_type    TEXT        NOT NULL,
    ticker        TEXT        NOT NULL,
    timeframe     TEXT        NOT NULL,
    connector_id  UUID,
    label         TEXT        NOT NULL DEFAULT '',
    -- Empty = inherit the color of the first hive holding it.
    color         TEXT        NOT NULL DEFAULT '',
    -- Cache of the last closed bar read (7.4).
    last_close    DOUBLE PRECISION,
    last_open     DOUBLE PRECISION,
    last_volume   DOUBLE PRECISION,
    avg_volume    DOUBLE PRECISION,
    prev_close    DOUBLE PRECISION,
    -- Open of the first bar of the bar's UTC day (the bar's own open from 1d up).
    day_open      DOUBLE PRECISION,
    last_bar_ts   TIMESTAMPTZ,
    -- Recent closes as [unix seconds, close] pairs, oldest first, for the sparkline and
    -- the lines view (the stamps place hits on the line).
    spark         JSONB       NOT NULL DEFAULT '[]'::jsonb,
    -- Whether each signal held on the last closed bar, keyed by signal id.
    signal_state  JSONB       NOT NULL DEFAULT '{}'::jsonb,
    checked_at    TIMESTAMPTZ,
    last_error    TEXT        NOT NULL DEFAULT '',
    updated_at    TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT scanner_items_instrument
        UNIQUE NULLS NOT DISTINCT (provider, ticker, timeframe, connector_id)
);

CREATE TABLE IF NOT EXISTS scanner_hives (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT        NOT NULL,
    color       TEXT        NOT NULL DEFAULT '',
    sort        INT         NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS scanner_hive_items (
    hive_id  UUID NOT NULL REFERENCES scanner_hives(id) ON DELETE CASCADE,
    item_id  UUID NOT NULL REFERENCES scanner_items(id) ON DELETE CASCADE,
    PRIMARY KEY (hive_id, item_id)
);
CREATE INDEX IF NOT EXISTS scanner_hive_items_item_idx ON scanner_hive_items (item_id);

-- A reusable condition. `config` depends on the builder: a backtest signal group, an event,
-- a candlestick pattern or structure, or a time window.
CREATE TABLE IF NOT EXISTS scanner_signals (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT        NOT NULL,
    builder     TEXT        NOT NULL CHECK (builder IN ('signal', 'event', 'pattern', 'time')),
    config      JSONB       NOT NULL DEFAULT '{}'::jsonb,
    color       TEXT        NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Which signals, where, and how to be told. Every signal must hold on the same closed bar;
-- OR is two alerts. `target_id` names an item or a hive (no foreign key: a deleted target is
-- an error the alert reports, not a silent cascade).
CREATE TABLE IF NOT EXISTS scanner_alerts (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name           TEXT        NOT NULL DEFAULT '',
    enabled        BOOLEAN     NOT NULL DEFAULT true,
    target_kind    TEXT        NOT NULL CHECK (target_kind IN ('item', 'hive')),
    target_id      UUID        NOT NULL,
    signal_ids     UUID[]      NOT NULL,
    notify         TEXT        NOT NULL DEFAULT 'digest' CHECK (notify IN ('digest', 'per_hit')),
    -- [] = every channel the scanner is granted.
    channels       JSONB       NOT NULL DEFAULT '[]'::jsonb,
    -- Silences repeats per item: no second hit on the same item within this.
    cooldown_secs  INT         NOT NULL DEFAULT 0,
    last_run_at    TIMESTAMPTZ,
    -- Why the last run could not work. Notified once per distinct value, and again when it
    -- clears.
    last_error     TEXT        NOT NULL DEFAULT '',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT scanner_alerts_signals CHECK (cardinality(signal_ids) > 0)
);

-- A hit is (alert, item, bar): the unique index makes a bar judged twice (a restart, a
-- second pass) one row and one notification.
CREATE TABLE IF NOT EXISTS scanner_hits (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    alert_id     UUID        NOT NULL REFERENCES scanner_alerts(id) ON DELETE CASCADE,
    item_id      UUID        NOT NULL REFERENCES scanner_items(id) ON DELETE CASCADE,
    bar_ts       TIMESTAMPTZ NOT NULL,
    "values"     JSONB       NOT NULL DEFAULT '{}'::jsonb,
    notified_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT scanner_hits_once UNIQUE (alert_id, item_id, bar_ts)
);
CREATE INDEX IF NOT EXISTS scanner_hits_created_idx ON scanner_hits (created_at DESC);
CREATE INDEX IF NOT EXISTS scanner_hits_item_idx ON scanner_hits (item_id, bar_ts DESC);

-- A saved view of the board over one hive.
CREATE TABLE IF NOT EXISTS scanner_views (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT        NOT NULL,
    kind        TEXT        NOT NULL CHECK (kind IN ('grid', 'honeycomb', 'lines')),
    hive_id     UUID        REFERENCES scanner_hives(id) ON DELETE CASCADE,
    color_by    JSONB       NOT NULL DEFAULT '{"metric":"change_pct"}'::jsonb,
    -- Size, live switch and view options, the client's own shape.
    layout      JSONB       NOT NULL DEFAULT '{}'::jsonb,
    sort        INT         NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
