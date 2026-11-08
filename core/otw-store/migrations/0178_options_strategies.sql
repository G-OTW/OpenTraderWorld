-- Options: saved multi-leg strategies. Legs are kept as the builder sent them (kind, strike,
-- expiry, signed qty, price, iv); the chain they were priced from is not stored.
CREATE TABLE IF NOT EXISTS options_strategies (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT        NOT NULL,
    underlying  TEXT        NOT NULL,
    asset_type  TEXT        NOT NULL,
    provider    TEXT,
    legs        JSONB       NOT NULL,
    notes       TEXT        NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS options_strategies_updated_idx ON options_strategies (updated_at DESC);
