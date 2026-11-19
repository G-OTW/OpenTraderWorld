-- Resources module.
--
-- A bookmarks library grouped by user-created master categories. Each resource has a
-- name, link, optional description, and belongs to one category. Single-user: no owner
-- scoping. Display mode (card/list) and sort direction are a frontend UI concern, not
-- persisted here.

CREATE TABLE IF NOT EXISTS resource_categories (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS resources (
    id          UUID PRIMARY KEY,
    category_id UUID NOT NULL REFERENCES resource_categories(id) ON DELETE CASCADE,
    name        TEXT NOT NULL DEFAULT '',
    link        TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_resources_category ON resources(category_id);

-- Seed a "Books" category with a few classic trading reads.
DO $$
DECLARE
    books_id UUID := gen_random_uuid();
BEGIN
    IF NOT EXISTS (SELECT 1 FROM resource_categories) THEN
        INSERT INTO resource_categories (id, name) VALUES (books_id, 'Books');
        INSERT INTO resources (id, category_id, name, link, description) VALUES
            (gen_random_uuid(), books_id, 'Market Wizards', '', 'Jack D. Schwager — interviews with top traders.'),
            (gen_random_uuid(), books_id, 'The New Market Wizards', '', 'Jack D. Schwager — more conversations with America''s top traders.'),
            (gen_random_uuid(), books_id, 'Stock Market Wizards', '', 'Jack D. Schwager — interviews with America''s top stock traders.'),
            (gen_random_uuid(), books_id, 'Hedge Fund Market Wizards', '', 'Jack D. Schwager — how winning traders win.'),
            (gen_random_uuid(), books_id, 'The Little Book of Market Wizards', '', 'Jack D. Schwager — lessons from the greatest traders.'),
            (gen_random_uuid(), books_id, 'Unknown Market Wizards', '', 'Jack D. Schwager — the best traders you''ve never heard of.'),
            (gen_random_uuid(), books_id, 'Trading in the Zone', '', 'Mark Douglas — mastering the market with confidence, discipline and a winning attitude.');
    END IF;
END $$;
