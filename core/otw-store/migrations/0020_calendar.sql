-- Calendar module — personal events.
--
-- The Economics and Earnings tabs are rendered from embedded investing.com widgets
-- (no data stored). Only the user's own personal events are persisted here.
-- Single-user: no owner scoping.

CREATE TABLE IF NOT EXISTS calendar_events (
    id          UUID PRIMARY KEY,
    title       TEXT NOT NULL DEFAULT '',
    -- Start/end as timestamps. For all-day events, time is midnight and all_day=TRUE;
    -- end_at is exclusive (matches FullCalendar semantics) and may be NULL.
    start_at    TIMESTAMPTZ NOT NULL,
    end_at      TIMESTAMPTZ,
    all_day     BOOLEAN NOT NULL DEFAULT FALSE,
    -- Free-form grouping label; empty string means uncategorized.
    category    TEXT NOT NULL DEFAULT '',
    -- Hex colour for the event chip; empty string falls back to the accent.
    color       TEXT NOT NULL DEFAULT '',
    location    TEXT NOT NULL DEFAULT '',
    notes       TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_calendar_events_start ON calendar_events(start_at);
