-- Editor module: documents organized as a folder tree.
--
-- A single table models both folders and pages (AFFiNE-like: everything is a doc).
-- `parent_id` builds the tree; `kind` distinguishes a container (folder) from a page.
-- Single-user install: no owner column, no sharing.

CREATE TABLE IF NOT EXISTS documents (
    id          UUID PRIMARY KEY,
    parent_id   UUID REFERENCES documents(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL DEFAULT 'page' CHECK (kind IN ('page', 'folder')),
    title       TEXT NOT NULL DEFAULT '',
    -- Rich content as TipTap/ProseMirror JSON. NULL for folders.
    content     JSONB,
    -- Manual ordering among siblings.
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    icon        TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_documents_parent ON documents(parent_id);
CREATE INDEX IF NOT EXISTS idx_documents_parent_position ON documents(parent_id, position);
