-- The user's own provider priority for a Fundamentals dataset (or `transcripts`), best
-- first. No row = the app's default order. Providers the app adds later are appended
-- after the stored ones on read.
CREATE TABLE IF NOT EXISTS fund_provider_order (
    dataset    TEXT PRIMARY KEY,
    providers  TEXT[] NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
