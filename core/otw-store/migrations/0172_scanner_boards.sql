-- Scanner boards: named sets of views. Every view belongs to one board; deleting a board
-- deletes its views. Views saved before boards existed move to one board named Main.
CREATE TABLE IF NOT EXISTS scanner_boards (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT        NOT NULL,
    sort        INT         NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE scanner_views ADD COLUMN IF NOT EXISTS board_id UUID REFERENCES scanner_boards(id) ON DELETE CASCADE;

DO $$
DECLARE b UUID;
BEGIN
    IF EXISTS (SELECT 1 FROM scanner_views WHERE board_id IS NULL) THEN
        INSERT INTO scanner_boards (name) VALUES ('Main') RETURNING id INTO b;
        UPDATE scanner_views SET board_id = b WHERE board_id IS NULL;
    END IF;
END $$;

ALTER TABLE scanner_views ALTER COLUMN board_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS scanner_views_board_idx ON scanner_views (board_id, sort);
