-- Per-source colour, shown as a dot on the source row and on every news item
-- it produced. NULL = no colour picked (the UI falls back to a neutral dot).
ALTER TABLE feeds ADD COLUMN color TEXT;
