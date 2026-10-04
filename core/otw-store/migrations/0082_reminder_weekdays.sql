-- Weekly reminders can pick the days they fire on.
--
-- Bitmask, Monday = 1 … Sunday = 64 (same convention as the routines scheduler).
-- NULL or 0 keeps the historical behaviour: every 7 days from `start_date`.
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS weekdays SMALLINT;
