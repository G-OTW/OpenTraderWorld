-- Community Docs: multiple categories + language.
--
-- Docs can now belong to several categories (e.g. "strategy" AND "risk") and carry a
-- language code so the library can group/filter by locale. Replaces the single `category`
-- TEXT column with a `categories TEXT[]`, backfilling each existing doc's lone category as
-- a one-element array (dropping empties). `language` defaults to 'en' for existing rows.
-- These fields are populated at publication time by the website sync. Single-user.

ALTER TABLE community_docs ADD COLUMN categories TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE community_docs ADD COLUMN language    TEXT   NOT NULL DEFAULT 'en';

-- Backfill categories[] from the old scalar column, skipping blank/absent values.
UPDATE community_docs
   SET categories = ARRAY[category]
 WHERE category IS NOT NULL AND category <> '';

ALTER TABLE community_docs DROP COLUMN category;
