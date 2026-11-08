-- Time Tracker module.
--
-- Track time against projects with live, server-side timers. A running timer is just a
-- time_entries row with no ended_at; elapsed = now() - started_at, so it survives a browser
-- close. Projects carry an optional time budget (hours) for 80/90/95/100% alerts and an
-- optional hourly rate to value the tracked time. Single-user: no owner scoping.

-- ── Projects ─────────────────────────────────────────────────────────────────
-- `time_budget_hours`: optional budget for alerting. `hourly_rate` (+ `rate_currency`):
-- optional value-per-hour; the breakdown multiplies tracked hours by it. `color` tints the
-- timer card.
CREATE TABLE IF NOT EXISTS time_projects (
    id                UUID PRIMARY KEY,
    name              TEXT NOT NULL DEFAULT '',
    category          TEXT,
    color             TEXT,
    planned_end       DATE,
    time_budget_hours DOUBLE PRECISION,
    hourly_rate       DOUBLE PRECISION,
    rate_currency     TEXT NOT NULL DEFAULT 'USD',
    archived          BOOLEAN NOT NULL DEFAULT FALSE,
    position          DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_time_projects_pos ON time_projects(position);

-- ── Time entries ─────────────────────────────────────────────────────────────
-- A tracked segment. `ended_at IS NULL` means the timer is currently running. At most one
-- open entry per project is enforced in the store layer (start is a no-op if one is open).
CREATE TABLE IF NOT EXISTS time_entries (
    id         UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES time_projects(id) ON DELETE CASCADE,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at   TIMESTAMPTZ,
    note       TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_time_entries_project ON time_entries(project_id, started_at);
-- At most one running entry per project.
CREATE UNIQUE INDEX IF NOT EXISTS idx_time_entries_one_open
    ON time_entries(project_id) WHERE ended_at IS NULL;

-- ── App state ────────────────────────────────────────────────────────────────
-- Single-row heartbeat: the client updates `last_seen_at` while the app is open so that on
-- reopen we can offer to revert a still-running timer back to when the tab was last seen
-- (discarding time accrued while the browser was closed). `display_currency` labels the
-- breakdown's monetary (hourly-rate) figures.
CREATE TABLE IF NOT EXISTS time_state (
    id               BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    last_seen_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    display_currency TEXT NOT NULL DEFAULT 'USD'
);
INSERT INTO time_state (id, last_seen_at)
SELECT TRUE, now()
WHERE NOT EXISTS (SELECT 1 FROM time_state);
