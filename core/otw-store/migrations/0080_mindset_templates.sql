-- Mindset templates: the same shape the Routines module gained in 0078/0079.
--
-- Prompts were a flat list split only by phase (pre | post), so a trader could not keep a
-- short daily check-in next to a long weekly review — everything landed in the same two
-- cards. A prompt now belongs to a **template**: a named, categorised set with its own
-- description and notes. `phase` stays on the template (it still orders the trading day).

CREATE TABLE mindset_categories (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '',
    position DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE mindset_templates (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    -- Rich text (HTML, sanitised at the API), shown when the check-in is unfolded.
    notes TEXT NOT NULL DEFAULT '',
    category_id UUID REFERENCES mindset_categories(id) ON DELETE SET NULL,
    -- pre (before the session) | post (after) — kept from the prompt row it replaces.
    phase TEXT NOT NULL DEFAULT 'pre',
    position DOUBLE PRECISION NOT NULL DEFAULT 0,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX mindset_templates_category ON mindset_templates(category_id);

ALTER TABLE mindset_prompts
    ADD COLUMN template_id UUID REFERENCES mindset_templates(id) ON DELETE CASCADE,
    -- Optional helper text under the prompt, for prompts that need framing.
    ADD COLUMN hint TEXT NOT NULL DEFAULT '';
CREATE INDEX mindset_prompts_template ON mindset_prompts(template_id);

-- One check-in per (date, template). The old uniqueness was (date, phase), which allowed
-- exactly one card per phase — the limitation this migration removes.
ALTER TABLE mindset_entries
    ADD COLUMN template_id UUID REFERENCES mindset_templates(id) ON DELETE CASCADE;

-- Consistency marks, same two verdicts as the routines board:
--   full   — checked in and followed through   (green)
--   action — traded without the check-in       (amber)
CREATE TABLE mindset_day_marks (
    day DATE PRIMARY KEY,
    mark TEXT NOT NULL CHECK (mark IN ('full', 'action')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Backfill ────────────────────────────────────────────────────────────────
-- One category and one template per phase actually in use, so an existing install keeps
-- exactly the two cards it had, now as editable templates.

INSERT INTO mindset_categories (id, name, color, position)
SELECT gen_random_uuid(), v.name, v.color, v.pos
FROM (VALUES
    ('pre',  'Before the session', 'var(--chart-1)', 0::double precision),
    ('post', 'After the session',  'var(--chart-3)', 1)
) AS v(phase, name, color, pos)
WHERE EXISTS (SELECT 1 FROM mindset_prompts p WHERE p.phase = v.phase);

INSERT INTO mindset_templates (id, name, category_id, phase, position)
SELECT gen_random_uuid(), v.name, c.id, v.phase, v.pos
FROM (VALUES
    ('pre',  'Pre-mortem',  'Before the session', 0::double precision),
    ('post', 'Post-mortem', 'After the session',  1)
) AS v(phase, name, cat, pos)
JOIN mindset_categories c ON c.name = v.cat
WHERE EXISTS (SELECT 1 FROM mindset_prompts p WHERE p.phase = v.phase);

UPDATE mindset_prompts p
SET template_id = t.id
FROM mindset_templates t
WHERE t.phase = p.phase AND p.template_id IS NULL;

UPDATE mindset_entries e
SET template_id = t.id
FROM mindset_templates t
WHERE t.phase = e.phase AND e.template_id IS NULL;

-- Entries whose phase has no template (none should exist) would otherwise block the index.
DELETE FROM mindset_entries WHERE template_id IS NULL;

ALTER TABLE mindset_entries
    ALTER COLUMN template_id SET NOT NULL,
    DROP CONSTRAINT mindset_entries_entry_date_phase_key;
CREATE UNIQUE INDEX mindset_entries_date_template ON mindset_entries(entry_date, template_id);

-- Prompts with no template are orphans from a partially-seeded install; nothing renders
-- them, so drop them rather than leave rows the UI can never reach.
DELETE FROM mindset_prompts WHERE template_id IS NULL;
ALTER TABLE mindset_prompts ALTER COLUMN template_id SET NOT NULL;
