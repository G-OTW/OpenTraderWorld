-- Routine templates: a routine becomes a named, reusable template with a category, a rich
-- recurrence (not just a weekday mask), and richer items (note + link per line).

-- ── Categories ───────────────────────────────────────────────────────────────
-- User-editable buckets replacing the fixed pre|live|post|any `session` column. The old
-- column stays for one release so existing rows/clients keep working; new writes set both.
CREATE TABLE trader_routine_categories (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    -- Free-form accent, one of the theme's chart tokens or a hex value.
    color TEXT NOT NULL DEFAULT '',
    position DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Template fields on the routine ───────────────────────────────────────────
ALTER TABLE trader_routines
    ADD COLUMN description TEXT NOT NULL DEFAULT '',
    -- Rich text (HTML, sanitised at the API) shown when the template is unfolded.
    ADD COLUMN notes TEXT NOT NULL DEFAULT '',
    ADD COLUMN category_id UUID REFERENCES trader_routine_categories(id) ON DELETE SET NULL,
    -- Recurrence as JSON so a rule can grow without a migration:
    --   {"kind":"weekly","weekdays":31,"interval":1}
    --   {"kind":"daily","interval":1}
    --   {"kind":"monthly","days":[1,15]}
    --   {"kind":"nth_weekday","weekday":3,"nth":1}   -- 1st Thursday, nth=-1 = last
    --   {"kind":"once","date":"2026-09-01"}
    -- NULL = fall back to the legacy `weekdays` mask.
    ADD COLUMN schedule JSONB,
    -- Optional active window; NULL ends open.
    ADD COLUMN start_date DATE,
    ADD COLUMN end_date DATE,
    ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── Richer checklist items ───────────────────────────────────────────────────
ALTER TABLE trader_routine_items
    -- Rich text (HTML, sanitised) — the "how" behind a checkbox, unfolded on click.
    ADD COLUMN note TEXT NOT NULL DEFAULT '',
    ADD COLUMN url TEXT NOT NULL DEFAULT '',
    ADD COLUMN link_label TEXT NOT NULL DEFAULT '';

CREATE INDEX trader_routines_category ON trader_routines(category_id);

-- ── Backfill: one category per legacy session bucket, only for buckets in use ─
INSERT INTO trader_routine_categories (id, name, color, position)
SELECT gen_random_uuid(), v.name, v.color, v.pos
FROM (VALUES
    ('pre',  'Pre-market',   'var(--chart-1)', 0::double precision),
    ('live', 'In session',   'var(--chart-2)', 1),
    ('post', 'Post-market',  'var(--chart-3)', 2),
    ('any',  'Anytime',      'var(--chart-4)', 3)
) AS v(session, name, color, pos)
WHERE EXISTS (SELECT 1 FROM trader_routines r WHERE r.session = v.session);

UPDATE trader_routines r
SET category_id = c.id
FROM trader_routine_categories c
WHERE c.name = CASE r.session
        WHEN 'pre' THEN 'Pre-market'
        WHEN 'live' THEN 'In session'
        WHEN 'post' THEN 'Post-market'
        ELSE 'Anytime'
    END
  AND r.category_id IS NULL;

-- Legacy masks become explicit weekly schedules so every routine reads the same way.
UPDATE trader_routines
SET schedule = jsonb_build_object('kind', 'weekly', 'weekdays', weekdays, 'interval', 1)
WHERE schedule IS NULL;
