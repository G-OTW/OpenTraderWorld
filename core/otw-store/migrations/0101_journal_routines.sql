-- Journal routines: a named discipline habit tracked per calendar day.
--
-- A routine belongs to one category (one trade journal) and runs over a period:
-- `start_date` inclusive, `end_date` inclusive or NULL for open-ended. The calendar
-- reads the two tables together: a day inside a routine's period owes a check, a row
-- in journal_routine_checks says it was performed. Absence of a row is "not performed",
-- so unchecking deletes rather than storing a false, and no day is ever pre-seeded.

CREATE TABLE IF NOT EXISTS journal_routines (
    id          UUID PRIMARY KEY,
    category_id UUID NOT NULL REFERENCES journal_categories(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT,
    start_date  DATE NOT NULL,
    -- NULL = unlimited: the routine has no planned end.
    end_date    DATE,
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT journal_routines_period CHECK (end_date IS NULL OR end_date >= start_date)
);
CREATE INDEX IF NOT EXISTS idx_journal_routines_cat
    ON journal_routines(category_id, start_date);

CREATE TABLE IF NOT EXISTS journal_routine_checks (
    routine_id UUID NOT NULL REFERENCES journal_routines(id) ON DELETE CASCADE,
    check_date DATE NOT NULL,
    done_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (routine_id, check_date)
);
CREATE INDEX IF NOT EXISTS idx_journal_routine_checks_date
    ON journal_routine_checks(check_date);
