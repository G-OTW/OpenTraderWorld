-- Reminders can repeat by the minute or the hour.
--
-- `frequency = 'interval'`: fires every `every_minutes` from the start date and time,
-- with no day boundary. NULL for every other frequency.
--
-- Repeat within the day (any other frequency): on each day the schedule fires, the first
-- fire is at `start_time`, then one every `repeat_every_minutes`, `repeat_count` fires in
-- all (the first included). Both NULL = one fire per day, as before.
ALTER TABLE reminders DROP CONSTRAINT IF EXISTS reminders_frequency_check;
ALTER TABLE reminders ADD CONSTRAINT reminders_frequency_check
    CHECK (frequency IN ('once', 'interval', 'daily', 'weekly', 'monthly', 'yearly'));
ALTER TABLE reminders
    ADD COLUMN IF NOT EXISTS every_minutes INTEGER,
    ADD COLUMN IF NOT EXISTS repeat_every_minutes INTEGER,
    ADD COLUMN IF NOT EXISTS repeat_count INTEGER;
