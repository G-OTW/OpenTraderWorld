-- Guards on manual trading from the charts: rules over the chart session's own trades, each
-- with what happens when it is reached (an alert, a window to acknowledge, the chart session
-- stopped until the user resets it, or the kill switch).

-- Who sent a leg: `session` (someone at a chart), `alert:<id>`, `strategy:<id>`. Rows written
-- before this column existed stay NULL and count as nobody's.
ALTER TABLE broker_orders ADD COLUMN source TEXT;

-- One rule. `account_id` NULL: every account, each one read on its own.
CREATE TABLE chart_guards (
    id              BIGSERIAL PRIMARY KEY,
    account_id      UUID REFERENCES broker_accounts(id) ON DELETE CASCADE,
    rule            JSONB NOT NULL,
    action          TEXT NOT NULL CHECK (action IN ('alert', 'acknowledge', 'stop', 'kill_switch')),
    -- With `stop`: the chart orders working are cancelled and the positions they opened closed.
    close_positions BOOLEAN NOT NULL DEFAULT FALSE,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- When a chart session ends on its own: `day` (the next day in `timezone`) or `manual` (only a
-- reset ends it). One row.
CREATE TABLE chart_guard_settings (
    id         BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    reset      TEXT NOT NULL DEFAULT 'day' CHECK (reset IN ('day', 'manual')),
    timezone   TEXT NOT NULL DEFAULT 'UTC',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Per account: the user's last reset, and the equity the current session started with.
CREATE TABLE chart_guard_sessions (
    account_id    UUID PRIMARY KEY REFERENCES broker_accounts(id) ON DELETE CASCADE,
    reset_at      TIMESTAMPTZ,
    session_start TIMESTAMPTZ,
    equity        NUMERIC,
    currency      TEXT
);

-- What fired. One row per guard, account, session and occurrence (`key`), so a rule read
-- again finds what it already did. A `stop` holds while its session is the current one and it
-- is not cleared.
CREATE TABLE chart_guard_events (
    id              BIGSERIAL PRIMARY KEY,
    guard_id        BIGINT REFERENCES chart_guards(id) ON DELETE SET NULL,
    account_id      UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    session_start   TIMESTAMPTZ NOT NULL,
    key             TEXT NOT NULL,
    action          TEXT NOT NULL,
    detail          TEXT NOT NULL,
    at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    acknowledged_at TIMESTAMPTZ,
    cleared_at      TIMESTAMPTZ,
    UNIQUE (guard_id, account_id, session_start, key)
);

CREATE INDEX idx_chart_guard_events_open ON chart_guard_events (account_id, session_start) WHERE cleared_at IS NULL;
