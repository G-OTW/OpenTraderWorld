-- Trading Journal module.
--
-- Log trades (typed reserved fields + free-form custom fields driven by templates),
-- organize them into categories (folders), and compute performance breakdowns per
-- category (equity curve, PnL, fees, Sharpe, win rate…).
--
-- Reserved fields live in typed columns so PnL/stats can be computed for every trade
-- regardless of template. Custom template fields live in `fields` (JSONB). Single-user:
-- no owner scoping (consistent with documents/databases/feeds).

-- ── Categories ───────────────────────────────────────────────────────────────
-- A category is a folder that trades sort into. One default category is seeded; it
-- can be renamed but not deleted. Capital (beginning stack + refills) is tracked per
-- category via journal_capital_events.
CREATE TABLE IF NOT EXISTS journal_categories (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL DEFAULT 'Default',
    color      TEXT,
    position   DOUBLE PRECISION NOT NULL DEFAULT 0,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_journal_categories_pos ON journal_categories(position);

-- Exactly one default category. Seed it (runs once with this migration).
INSERT INTO journal_categories (id, name, position, is_default)
SELECT gen_random_uuid(), 'Default', 0, TRUE
WHERE NOT EXISTS (SELECT 1 FROM journal_categories WHERE is_default);

-- ── Capital events ───────────────────────────────────────────────────────────
-- The beginning stack and any later refills (fresh capital added to an existing
-- category). The category's invested capital at a point in time is the sum of events
-- up to that time; the equity curve and return metrics are computed against it.
-- `kind`: 'initial' (beginning stack) | 'refill' (fresh capital) | 'withdrawal'.
CREATE TABLE IF NOT EXISTS journal_capital_events (
    id          UUID PRIMARY KEY,
    category_id UUID NOT NULL REFERENCES journal_categories(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL DEFAULT 'refill'
                CHECK (kind IN ('initial', 'refill', 'withdrawal')),
    amount      DOUBLE PRECISION NOT NULL DEFAULT 0,
    note        TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_journal_capital_cat ON journal_capital_events(category_id, occurred_at);

-- ── Strategies ───────────────────────────────────────────────────────────────
-- A named strategy with optional signal names. Trades reference a strategy by id;
-- the trade also stores the resolved strategy/signal name for display stability.
CREATE TABLE IF NOT EXISTS journal_strategies (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL DEFAULT '',
    description TEXT,
    -- Signal names offered for trades using this strategy: ["Breakout", "Pullback", …].
    signals     JSONB NOT NULL DEFAULT '[]'::jsonb,
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_journal_strategies_pos ON journal_strategies(position);

-- ── Templates ────────────────────────────────────────────────────────────────
-- A template defines the form used to log a trade: the ordered list of fields.
-- Reserved fields (ticker, side, quantity, entry/exit price, fees, leverage, …) map
-- onto typed columns on journal_trades so stats can be computed. Custom fields are
-- stored in journal_trades.fields.
--
-- `fields` is an ordered array of:
--   { "key": "entry_price", "label": "Entry price", "type": "number",
--     "reserved": "entry_price"|null, "options": {...} }
-- `reserved` names which typed trade column the field feeds (null = custom field).
-- A grid layout (column span / order) can live in `options.grid`.
CREATE TABLE IF NOT EXISTS journal_templates (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL DEFAULT 'Untitled template',
    description TEXT,
    fields      JSONB NOT NULL DEFAULT '[]'::jsonb,
    is_builtin  BOOLEAN NOT NULL DEFAULT FALSE,
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_journal_templates_pos ON journal_templates(position);

-- ── Trades ───────────────────────────────────────────────────────────────────
-- Reserved typed columns power computed stats. `side` is 'long'|'short'. PnL is
-- computed (not stored) so edits stay consistent:
--   gross = (exit_price - entry_price) * quantity * (side=long ? 1 : -1)
--   net   = gross - fees
-- `multiplier` covers options contract size; `leverage` is informational margin context.
-- Custom template fields live in `fields` (JSONB, keyed by field key).
-- `images` is an array of file ids (uploads), max 2 enforced in the API.
CREATE TABLE IF NOT EXISTS journal_trades (
    id            UUID PRIMARY KEY,
    category_id   UUID NOT NULL REFERENCES journal_categories(id) ON DELETE CASCADE,
    template_id   UUID REFERENCES journal_templates(id) ON DELETE SET NULL,
    strategy_id   UUID REFERENCES journal_strategies(id) ON DELETE SET NULL,

    ticker        TEXT NOT NULL DEFAULT '',
    -- asset class: stock | option | crypto | etf | future | forex | other
    asset_class   TEXT NOT NULL DEFAULT 'stock',
    exchange      TEXT,
    side          TEXT NOT NULL DEFAULT 'long' CHECK (side IN ('long', 'short')),

    entry_at      TIMESTAMPTZ,
    exit_at       TIMESTAMPTZ,
    entry_price   DOUBLE PRECISION,
    exit_price    DOUBLE PRECISION,
    quantity      DOUBLE PRECISION,
    fees          DOUBLE PRECISION NOT NULL DEFAULT 0,
    leverage      DOUBLE PRECISION NOT NULL DEFAULT 1,
    multiplier    DOUBLE PRECISION NOT NULL DEFAULT 1,

    signal_name   TEXT,
    feedback      TEXT,
    images        JSONB NOT NULL DEFAULT '[]'::jsonb,
    fields        JSONB NOT NULL DEFAULT '{}'::jsonb,

    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_journal_trades_cat ON journal_trades(category_id, exit_at);
CREATE INDEX IF NOT EXISTS idx_journal_trades_strategy ON journal_trades(strategy_id);
