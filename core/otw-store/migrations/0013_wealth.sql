-- MyWealth module.
--
-- Track net worth across asset types. Assets are described by templates (reserved fields
-- like price/quantity + custom fields, same shape as the journal's templates). Each "Update"
-- inserts a new revision with the valued amount, building a time series. The net-worth
-- breakdown sums each asset's latest revision on/before a date, FX-converted to a display
-- currency. Single-user: no owner scoping.

-- ── Asset templates ──────────────────────────────────────────────────────────
-- `asset_type`: money | stock | crypto | watch | house | vehicle | other.
-- `fields`: ordered array, same shape as journal_templates.fields:
--   { "key","label","type","reserved":"price"|"quantity"|null,"options":{...} }
CREATE TABLE IF NOT EXISTS wealth_templates (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL DEFAULT 'Untitled template',
    description TEXT,
    asset_type  TEXT NOT NULL DEFAULT 'other',
    fields      JSONB NOT NULL DEFAULT '[]'::jsonb,
    is_builtin  BOOLEAN NOT NULL DEFAULT FALSE,
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_wealth_templates_pos ON wealth_templates(position);

-- ── Assets ───────────────────────────────────────────────────────────────────
-- The asset identity; its current value is the latest revision. `currency` is the asset's
-- native currency; revisions are converted at read time.
CREATE TABLE IF NOT EXISTS wealth_assets (
    id          UUID PRIMARY KEY,
    template_id UUID REFERENCES wealth_templates(id) ON DELETE SET NULL,
    name        TEXT NOT NULL DEFAULT '',
    asset_type  TEXT NOT NULL DEFAULT 'other',
    currency    TEXT NOT NULL DEFAULT 'USD',
    category    TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Revisions ────────────────────────────────────────────────────────────────
-- Each update inserts a revision. `value` is the asset's worth at `valued_at` in the
-- asset's currency: price×quantity when both are set, else the entered amount. `fields`
-- holds custom template values for that revision.
CREATE TABLE IF NOT EXISTS wealth_revisions (
    id         UUID PRIMARY KEY,
    asset_id   UUID NOT NULL REFERENCES wealth_assets(id) ON DELETE CASCADE,
    valued_at  DATE NOT NULL DEFAULT CURRENT_DATE,
    price      DOUBLE PRECISION,
    quantity   DOUBLE PRECISION,
    value      DOUBLE PRECISION NOT NULL DEFAULT 0,
    fields     JSONB NOT NULL DEFAULT '{}'::jsonb,
    note       TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_wealth_revisions_asset ON wealth_revisions(asset_id, valued_at);

-- ── Settings ─────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS wealth_settings (
    id               BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    display_currency TEXT NOT NULL DEFAULT 'USD',
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO wealth_settings (id, display_currency)
SELECT TRUE, 'USD'
WHERE NOT EXISTS (SELECT 1 FROM wealth_settings);
