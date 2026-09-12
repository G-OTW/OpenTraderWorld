-- Routines become a library, and a link is what puts one on a calendar.
--
-- 0101 tied a routine to one book and carried its period on the routine row, so the
-- same habit had to be retyped for every journal. A routine is now just a named habit
-- (name + description); `journal_routine_links` attaches it to a category over a
-- period (`start_date` inclusive, `end_date` inclusive or NULL = unlimited). Two books
-- can therefore run the same routine over different periods.
--
-- A check hangs off the link, not the routine: the same habit ticked on one book must
-- not tick itself on another. The row's existence is still the whole value, so
-- unchecking deletes and no day is ever pre-seeded.

CREATE TABLE IF NOT EXISTS journal_routine_links (
    id          UUID PRIMARY KEY,
    routine_id  UUID NOT NULL REFERENCES journal_routines(id) ON DELETE CASCADE,
    category_id UUID NOT NULL REFERENCES journal_categories(id) ON DELETE CASCADE,
    start_date  DATE NOT NULL,
    end_date    DATE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT journal_routine_links_period CHECK (end_date IS NULL OR end_date >= start_date),
    -- One routine sits on a book once: a second period for the same habit would make
    -- the day's dot ambiguous.
    CONSTRAINT journal_routine_links_once UNIQUE (routine_id, category_id)
);
CREATE INDEX IF NOT EXISTS idx_journal_routine_links_cat
    ON journal_routine_links(category_id, start_date);

-- Carry the 0101 rows over: each routine becomes a library entry plus one link.
INSERT INTO journal_routine_links (id, routine_id, category_id, start_date, end_date)
SELECT gen_random_uuid(), id, category_id, start_date, end_date
FROM journal_routines
ON CONFLICT (routine_id, category_id) DO NOTHING;

-- Re-key the checks onto the link.
ALTER TABLE journal_routine_checks
    ADD COLUMN IF NOT EXISTS link_id UUID REFERENCES journal_routine_links(id) ON DELETE CASCADE;
UPDATE journal_routine_checks c
   SET link_id = l.id
  FROM journal_routine_links l
 WHERE l.routine_id = c.routine_id AND c.link_id IS NULL;
DELETE FROM journal_routine_checks WHERE link_id IS NULL;
ALTER TABLE journal_routine_checks DROP CONSTRAINT IF EXISTS journal_routine_checks_pkey;
ALTER TABLE journal_routine_checks DROP COLUMN IF EXISTS routine_id;
ALTER TABLE journal_routine_checks ALTER COLUMN link_id SET NOT NULL;
ALTER TABLE journal_routine_checks ADD PRIMARY KEY (link_id, check_date);

-- The period and the book now live on the link.
ALTER TABLE journal_routines
    DROP COLUMN IF EXISTS category_id,
    DROP COLUMN IF EXISTS start_date,
    DROP COLUMN IF EXISTS end_date;
