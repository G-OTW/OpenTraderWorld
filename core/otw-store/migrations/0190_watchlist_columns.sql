-- Watchlists: the columns a list shows, picked per list from the table header gear.
--
-- Ids come from the fixed catalog in `watchlists_api::COLUMNS` (validated there). The
-- default keeps the table every list had before, plus the 52-week position bar.
ALTER TABLE watchlists
    ADD COLUMN IF NOT EXISTS columns TEXT[] NOT NULL
        DEFAULT ARRAY['c24', 'c3', 'c7', 'c30', 'spark', 'range52', 'notes']::text[];
