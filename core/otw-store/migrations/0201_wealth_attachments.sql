-- MyWealth: a deed, a contract or a statement kept with the asset it proves.
--
-- Bytes live in the database so a backup of the database is a backup of the documents. The
-- 10 MB cap is enforced by the API; `size` is stored so a listing never reads the bytes.
CREATE TABLE IF NOT EXISTS wealth_attachments (
    id          UUID PRIMARY KEY,
    asset_id    UUID NOT NULL REFERENCES wealth_assets(id) ON DELETE CASCADE,
    revision_id UUID REFERENCES wealth_revisions(id) ON DELETE SET NULL,
    name        TEXT NOT NULL,
    mime        TEXT NOT NULL DEFAULT 'application/octet-stream',
    bytes       BYTEA NOT NULL,
    size        INTEGER NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_wealth_attachments_asset ON wealth_attachments(asset_id);
