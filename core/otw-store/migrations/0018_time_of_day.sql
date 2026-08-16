-- Time-of-day support for ToDo and RemindMe.
--
-- ToDo: an optional clock time shown alongside the due date (display only).
-- RemindMe: the wall-clock time a reminder fires, plus the browser's UTC offset (in
-- minutes) captured when it was saved, so "09:00 local" maps to the right UTC instant.
-- Existing reminders keep firing at midnight: start_time defaults to 00:00 and offset 0.

ALTER TABLE todos     ADD COLUMN IF NOT EXISTS due_time TIME;
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS start_time TIME NOT NULL DEFAULT '00:00';
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS tz_offset_minutes INTEGER NOT NULL DEFAULT 0;
