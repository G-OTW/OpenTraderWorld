-- Editor module: per-page content width. 'normal' = centered column,
-- 'wide' = full width. Applies to pages; folders ignore it.
ALTER TABLE documents
    ADD COLUMN IF NOT EXISTS layout TEXT NOT NULL DEFAULT 'normal'
        CHECK (layout IN ('normal', 'wide'));
