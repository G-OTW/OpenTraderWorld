-- A reminder can carry one link (the chart, the dashboard, the doc it is about).
--
-- The notification copies it at fire time — like `kind`/`linked_id` — so an already-fired
-- notification keeps working after the reminder is edited or deleted.
ALTER TABLE reminders
    ADD COLUMN IF NOT EXISTS url TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS link_label TEXT NOT NULL DEFAULT '';

ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS url TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS link_label TEXT NOT NULL DEFAULT '';
