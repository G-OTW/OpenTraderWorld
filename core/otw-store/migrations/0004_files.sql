-- Uploaded files (images embedded in documents, database covers, etc.).
--
-- Bytes live on disk under the core service's upload dir (one file per id);
-- this table holds the metadata and is the source of truth for what exists.
CREATE TABLE IF NOT EXISTS files (
    id           UUID PRIMARY KEY,
    filename     TEXT NOT NULL DEFAULT '',
    content_type TEXT NOT NULL DEFAULT 'application/octet-stream',
    size         BIGINT NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
