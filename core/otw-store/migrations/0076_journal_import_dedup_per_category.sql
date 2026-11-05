-- Import de-duplication is scoped to the category, not to the whole journal.
--
-- The first index was global, so the same statement could never be imported into two
-- categories: the second one counted every row as a duplicate. A category is the book
-- being kept — the same fills belonging to two books is the user's call, and re-importing
-- a file into the book it already went to is what has to stay a no-op.
DROP INDEX IF EXISTS idx_journal_trades_row_hash;
CREATE UNIQUE INDEX IF NOT EXISTS idx_journal_trades_cat_row_hash
    ON journal_trades(category_id, import_row_hash) WHERE import_row_hash IS NOT NULL;
