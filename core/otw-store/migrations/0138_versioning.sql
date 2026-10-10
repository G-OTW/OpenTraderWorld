-- Opt-in versioning for editor documents (pages and databases) and saved backtest strategies.
--
-- Versioning is switched on twice: globally in Settings (`app_settings` keys
-- `versioning_editor` / `versioning_strategies`), then per item through `versioned` below.
-- A version is a named snapshot the user takes on purpose, with an optional note.
--
-- Version rows carry the item id without a foreign key: the user may delete a document or a
-- strategy and keep its history, which is then listed as deleted and can be restored.
--
-- Uploaded media is never copied: page content references uploads by URL, so a snapshot
-- holds the JSON only.

ALTER TABLE documents ADD COLUMN IF NOT EXISTS versioned BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE backtest_strategies ADD COLUMN IF NOT EXISTS versioned BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE IF NOT EXISTS document_versions (
    id          UUID PRIMARY KEY,
    document_id UUID NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('page', 'database')),
    title       TEXT NOT NULL DEFAULT '',
    icon        TEXT,
    layout      TEXT NOT NULL DEFAULT 'normal',
    -- Page: the rich-text JSON. Database: its view config.
    content     JSONB,
    -- Database only: { columns: [...], rows: [...] } with their original ids.
    data        JSONB,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_document_versions_doc ON document_versions(document_id, created_at DESC);

CREATE TABLE IF NOT EXISTS strategy_versions (
    id          UUID PRIMARY KEY,
    strategy_id UUID NOT NULL,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    tags        TEXT[] NOT NULL DEFAULT '{}',
    settings    JSONB NOT NULL,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_strategy_versions_strategy ON strategy_versions(strategy_id, created_at DESC);
