-- Paper trading: a strategy left running forward on its own schedule.
--
-- A session is a *frozen* copy of the settings a backtest produced (editing the strategy
-- afterwards must not silently change what is being traded) plus the datasets it reads, a
-- recurrence rule and how the result is reported.
--
-- A tick re-runs the ordinary engine over the moving window and diffs the resulting trades
-- against `paper_trades`: there is no second engine and no serialized simulator state, so a
-- paper fill is by construction the fill the backtest would have shown. The natural key of a
-- trade is (ticker, direction, entry_ts), which the engine reproduces identically as long as
-- the bars before it do not change.
--
-- `paper_events` is both the notification log and the offline inbox: an event whose
-- `notified_at` is NULL was never delivered (engine down, no channel granted, digest still
-- pending), and one whose `seen_at` is NULL has not been acknowledged in the app yet.

CREATE TABLE IF NOT EXISTS paper_sessions (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL,
    -- The strategy it was started from, for provenance only: `settings` is the source of
    -- truth and does not follow later edits of that strategy.
    strategy_id     UUID REFERENCES backtest_strategies(id) ON DELETE SET NULL,
    settings        JSONB NOT NULL,
    dataset_ids     UUID[] NOT NULL DEFAULT '{}',

    -- Moving window fed to the engine. `window_bars` trails the present; `start_ts` pins an
    -- absolute start instead. One of the two, never both.
    window_bars     INT,
    start_ts        TIMESTAMPTZ,

    -- Where the bars come from. 'hist' walks the stored catalog on a schedule; 'live'
    -- subscribes to the live hub and re-simulates on each closed bar (no recurrence at all).
    -- The column exists from the start so adding the live path is code, not a migration over
    -- a table that already holds sessions.
    feed            TEXT NOT NULL DEFAULT 'hist',

    -- Recurrence, same vocabulary and same resolver as automator_schedules. Read only by a
    -- 'hist' session: a live one is driven by its feed.
    kind            TEXT NOT NULL DEFAULT 'interval',
    timezone        TEXT NOT NULL DEFAULT 'UTC',
    every_minutes   INT,
    at_hour         INT,
    at_minute       INT,
    weekdays        SMALLINT,
    day_of_month    INT,
    run_at          TIMESTAMPTZ,
    next_run_at     TIMESTAMPTZ,
    last_run_at     TIMESTAMPTZ,

    -- Delivery. `notify_when`: always | on_change | never. `cadence`: each | hourly | daily
    -- | weekly (a digest watermark, so a two-minute schedule does not mean a two-minute ping).
    notify_when     TEXT NOT NULL DEFAULT 'on_change',
    cadence         TEXT NOT NULL DEFAULT 'each',
    -- Empty = every channel the paper module is granted; a list narrows it.
    channel_ids     UUID[] NOT NULL DEFAULT '{}',
    -- User-authored message. Empty = the built-in default, so a blank template is never silence.
    title_template  TEXT NOT NULL DEFAULT '',
    body_template   TEXT NOT NULL DEFAULT '',
    last_digest_at  TIMESTAMPTZ,

    status          TEXT NOT NULL DEFAULT 'active',
    -- Hash of (settings, datasets, window, engine version, dataset freshness). A tick whose
    -- hash is unchanged has nothing new to simulate and returns without running the engine.
    inputs_hash     TEXT NOT NULL DEFAULT '',
    engine_version  INT NOT NULL DEFAULT 0,
    -- Last tick's stat block, for the list and for the message template.
    stats           JSONB,
    bars            INT NOT NULL DEFAULT 0,
    last_error      TEXT NOT NULL DEFAULT '',
    -- Set while a tick is in flight: a due occurrence arriving over a still-running tick is
    -- dropped, never queued, so a short interval on a long run cannot pile up.
    running         BOOLEAN NOT NULL DEFAULT false,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT paper_sessions_feed CHECK (feed IN ('hist', 'live')),
    CONSTRAINT paper_sessions_status CHECK (status IN ('active', 'paused', 'error')),
    CONSTRAINT paper_sessions_notify CHECK (notify_when IN ('always', 'on_change', 'never')),
    CONSTRAINT paper_sessions_cadence CHECK (cadence IN ('each', 'hourly', 'daily', 'weekly'))
);
CREATE INDEX IF NOT EXISTS idx_paper_sessions_due
    ON paper_sessions(next_run_at) WHERE status = 'active' AND feed = 'hist';

CREATE TABLE IF NOT EXISTS paper_trades (
    session_id  UUID NOT NULL REFERENCES paper_sessions(id) ON DELETE CASCADE,
    -- ticker|direction|entry_ts, the engine's own identity for a round trip.
    trade_key   TEXT NOT NULL,
    ticker      TEXT NOT NULL DEFAULT '',
    direction   TEXT NOT NULL,
    entry_ts    TEXT NOT NULL,
    exit_ts     TEXT NOT NULL DEFAULT '',
    -- An open position is the engine's trade closed at the last bar (exit_reason "end"),
    -- marked to that close. It becomes closed once a real exit reason replaces it.
    open        BOOLEAN NOT NULL DEFAULT true,
    trade       JSONB NOT NULL,
    opened_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at   TIMESTAMPTZ,
    PRIMARY KEY (session_id, trade_key)
);
CREATE INDEX IF NOT EXISTS idx_paper_trades_open
    ON paper_trades(session_id, open, entry_ts DESC);

CREATE TABLE IF NOT EXISTS paper_events (
    id          UUID PRIMARY KEY,
    session_id  UUID NOT NULL REFERENCES paper_sessions(id) ON DELETE CASCADE,
    at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- open | close | error | run
    kind        TEXT NOT NULL,
    payload     JSONB NOT NULL DEFAULT '{}',
    title       TEXT NOT NULL DEFAULT '',
    message     TEXT NOT NULL DEFAULT '',
    notified_at TIMESTAMPTZ,
    seen_at     TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_paper_events_session ON paper_events(session_id, at DESC);
CREATE INDEX IF NOT EXISTS idx_paper_events_unsent ON paper_events(at) WHERE notified_at IS NULL;
