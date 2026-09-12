-- Consistency marks: one deliberate verdict per day, set by the user.
--
-- Item ticks already record *what* was done, but they cannot say whether the day itself
-- counted: a trader can follow the routine and not trade, or trade without following it.
-- That judgement is the user's, so it is stored rather than derived.
--
--   full   — acted and followed the routine   (green)
--   action — acted without following it       (amber)
--
-- Absent row = nothing claimed for that day.

CREATE TABLE trader_day_marks (
    day DATE PRIMARY KEY,
    mark TEXT NOT NULL CHECK (mark IN ('full', 'action')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
