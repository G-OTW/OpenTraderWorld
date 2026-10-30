-- Databases for the editor module (AFFiNE-like: typed tables with multiple views).
--
-- A "database" is just a document with kind = 'database'; it lives in the same
-- tree as folders and pages (so it sorts, nests, and drags identically). Its
-- columns and rows are stored relationally here; its view configuration
-- (table / kanban / gallery settings) is stored in documents.content (JSONB).

-- Allow the new kind on the existing documents table.
ALTER TABLE documents DROP CONSTRAINT IF EXISTS documents_kind_check;
ALTER TABLE documents
    ADD CONSTRAINT documents_kind_check
    CHECK (kind IN ('page', 'folder', 'database'));

-- Column definitions for a database.
-- `type` is one of: text, number, select, multi_select, date, checkbox, url.
-- `options` holds type-specific config, e.g. select choices: {"choices":[{"id","name","color"}]}.
CREATE TABLE IF NOT EXISTS database_columns (
    id          UUID PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    name        TEXT NOT NULL DEFAULT '',
    type        TEXT NOT NULL DEFAULT 'text'
                CHECK (type IN ('text', 'number', 'select', 'multi_select', 'date', 'checkbox', 'url')),
    options     JSONB NOT NULL DEFAULT '{}'::jsonb,
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_database_columns_doc ON database_columns(document_id, position);

-- Rows. `cells` maps column_id -> value (shape depends on the column type):
--   text/url     -> "string"
--   number       -> 12.5
--   select       -> "optionId"
--   multi_select -> ["optionId", ...]
--   date         -> "2026-06-27"
--   checkbox     -> true
CREATE TABLE IF NOT EXISTS database_rows (
    id          UUID PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    cells       JSONB NOT NULL DEFAULT '{}'::jsonb,
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_database_rows_doc ON database_rows(document_id, position);
