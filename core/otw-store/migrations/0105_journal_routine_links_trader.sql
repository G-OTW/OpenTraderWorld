-- The journal calendar reads the Trading routines module, it does not keep its own.
--
-- 0101/0104 gave the journal a routine library of its own, which was a second place to
-- write the same habit and a second place to tick it. A routine is what
-- `trader_routines` already holds, ticked item by item in `trader_routine_checks`, so
-- the journal keeps exactly one thing: which routines a book runs, and over which
-- period. A day's colour is then derived, never stored.

DROP TABLE IF EXISTS journal_routine_checks;
DROP TABLE IF EXISTS journal_routine_links;
DROP TABLE IF EXISTS journal_routines;

CREATE TABLE journal_routine_links (
    id          UUID PRIMARY KEY,
    routine_id  UUID NOT NULL REFERENCES trader_routines(id) ON DELETE CASCADE,
    category_id UUID NOT NULL REFERENCES journal_categories(id) ON DELETE CASCADE,
    -- The window this book runs the routine over; NULL end = unlimited. The routine's
    -- own recurrence still decides which days inside it are actually due.
    start_date  DATE NOT NULL,
    end_date    DATE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT journal_routine_links_period CHECK (end_date IS NULL OR end_date >= start_date),
    -- One routine sits on a book once: a second period for the same habit would make
    -- the day's dot ambiguous.
    CONSTRAINT journal_routine_links_once UNIQUE (routine_id, category_id)
);
CREATE INDEX idx_journal_routine_links_cat
    ON journal_routine_links(category_id, start_date);
