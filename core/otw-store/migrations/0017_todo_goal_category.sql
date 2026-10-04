-- Add a free-text category to todos and goals, used for grouping/filtering in the UI.
-- Single-user; empty string means "uncategorized".

ALTER TABLE todos ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT '';
ALTER TABLE goals ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT '';
