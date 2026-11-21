-- Trading Robots: the paper sessions become a module of their own.
--
-- Nothing is copied: a session is a robot, its trades, events, orders and fills stay where
-- they are, and the new columns default to what a session always was (priority 100, no
-- capital policy, no guard, scale 1). Table names stay internal; the API and the UI say robot.
--
-- `priority`: lower runs first when several robots are due at once or compete for one venue.
-- `capital`: `{ allocation, currency, percent_in, max_positions, max_orders_per_day }`, applied
-- where the engine's decision becomes an order or a recorded robot trade, never in the engine.
-- `guards`: the rules over the robot's own closed trades, `[{ when, then }]`.
-- `guard_state`: what the guards hold now: `{ halted_until, halted_by, scale, scale_restore,
-- triggered_at, history_cursor, marks }`, `scale` 1 being normal.
-- `strategy_version`, `strategy_name`: which saved strategy was frozen, as it read then.
ALTER TABLE paper_sessions
    ADD COLUMN priority         INT NOT NULL DEFAULT 100,
    ADD COLUMN capital          JSONB NOT NULL DEFAULT '{}',
    ADD COLUMN guards           JSONB NOT NULL DEFAULT '[]',
    ADD COLUMN guard_state      JSONB NOT NULL DEFAULT '{}',
    ADD COLUMN strategy_version INT,
    ADD COLUMN strategy_name    TEXT NOT NULL DEFAULT '';

CREATE INDEX idx_paper_sessions_priority_due ON paper_sessions (priority, next_run_at)
    WHERE status = 'active' AND NOT running;

-- The size the robot actually took on a trade: the engine's size times the guard scale in
-- force at entry, cut by the capital policy. NULL on rows written before robots (read as the
-- engine's own size). `robot_note` says why a trade was not taken (`robot_qty` 0).
ALTER TABLE paper_trades
    ADD COLUMN robot_qty  NUMERIC,
    ADD COLUMN scale      NUMERIC NOT NULL DEFAULT 1,
    ADD COLUMN robot_note TEXT NOT NULL DEFAULT '';

-- Every guard trigger and every restoration: the dashboard's guard timeline.
CREATE TABLE robot_guard_events (
    id         UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES paper_sessions(id) ON DELETE CASCADE,
    at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    rule       JSONB NOT NULL DEFAULT '{}',
    action     JSONB NOT NULL DEFAULT '{}',
    trigger    JSONB NOT NULL DEFAULT '{}',
    cleared_at TIMESTAMPTZ
);

CREATE INDEX idx_robot_guard_events_session ON robot_guard_events (session_id, at DESC);

-- Per-day counters, so the order cap and a daily-loss guard read one row, not every fill.
CREATE TABLE robot_daily (
    session_id  UUID NOT NULL REFERENCES paper_sessions(id) ON DELETE CASCADE,
    day         DATE NOT NULL,
    orders_sent INT NOT NULL DEFAULT 0,
    realized    DOUBLE PRECISION NOT NULL DEFAULT 0,
    fees        DOUBLE PRECISION NOT NULL DEFAULT 0,
    PRIMARY KEY (session_id, day)
);

-- Grants follow the module: an account or a channel granted to Backtest for its paper
-- sessions is granted to Robots too, so nothing that was armed stops working.
INSERT INTO broker_account_modules (account_id, module)
SELECT account_id, 'robots' FROM broker_account_modules WHERE module = 'backtest'
ON CONFLICT DO NOTHING;

INSERT INTO channel_modules (channel_id, module)
SELECT channel_id, 'robots' FROM channel_modules WHERE module = 'backtest'
ON CONFLICT DO NOTHING;

-- An installation that lists its modules keeps Robots visible wherever Backtest was: the paper
-- sessions it ran from Backtest are robots now. A list that does not parse is left alone (the
-- app reads it as every module).
DO $$
BEGIN
    UPDATE app_settings
       SET value = (value::jsonb || '["robots"]'::jsonb)::text, updated_at = now()
     WHERE key = 'installed_modules'
       AND value::jsonb ? 'backtest'
       AND NOT (value::jsonb ? 'robots');
EXCEPTION WHEN others THEN
    NULL;
END $$;
