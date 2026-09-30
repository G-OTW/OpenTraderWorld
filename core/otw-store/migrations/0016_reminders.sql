-- RemindMe module.
--
-- Reminders fire in-app notifications on a cadence. A reminder can be linked to a goal,
-- a todo, or be fully custom. A background tick (every ~minute) finds reminders whose
-- `next_fire_at` is due, writes a `notifications` row, advances `next_fire_at` by the
-- frequency, and increments `fired_count` — stopping at `end_date` or `max_count`.
-- Single-user: no owner scoping.

CREATE TABLE IF NOT EXISTS reminders (
    id           UUID PRIMARY KEY,
    name         TEXT NOT NULL DEFAULT '',
    -- 'goal' | 'todo' | 'custom'. Custom reminders carry no linked_id.
    kind         TEXT NOT NULL DEFAULT 'custom'
                 CHECK (kind IN ('goal', 'todo', 'custom')),
    -- Soft reference to a goal/todo id (no FK: target may be deleted independently).
    linked_id    UUID,
    details      TEXT NOT NULL DEFAULT '',
    -- 'once' | 'daily' | 'weekly' | 'monthly' | 'yearly'.
    frequency    TEXT NOT NULL DEFAULT 'once'
                 CHECK (frequency IN ('once', 'daily', 'weekly', 'monthly', 'yearly')),
    start_date   DATE NOT NULL,
    end_date     DATE,
    -- NULL = unlimited; otherwise stop after this many fires.
    max_count    INTEGER,
    fired_count  INTEGER NOT NULL DEFAULT 0,
    -- When the next notification is due; NULL once the reminder is exhausted/finished.
    next_fire_at TIMESTAMPTZ,
    active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_reminders_due ON reminders(next_fire_at) WHERE next_fire_at IS NOT NULL;

-- Notifications produced by reminders firing. `read_at` drives the unread pastille/badge.
CREATE TABLE IF NOT EXISTS notifications (
    id          UUID PRIMARY KEY,
    reminder_id UUID REFERENCES reminders(id) ON DELETE SET NULL,
    name        TEXT NOT NULL DEFAULT '',
    -- 'goal' | 'todo' | 'custom' (copied from the reminder so links survive its deletion).
    kind        TEXT NOT NULL DEFAULT 'custom',
    linked_id   UUID,
    details     TEXT NOT NULL DEFAULT '',
    read_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_notifications_created ON notifications(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications(read_at) WHERE read_at IS NULL;
