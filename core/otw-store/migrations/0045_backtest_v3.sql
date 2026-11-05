-- Backtest v3: named strategies + custom indicators (the expert-mode library).
--
-- Two first-class libraries the expert-mode builder edits:
--   * backtest_strategies — a named, reusable `Settings` object (the same JSON the engine and
--     the normal-mode settings pane already consume). Runs snapshot their settings as before;
--     a run additionally records `strategy_id` as provenance (never a live link — editing a
--     strategy must not change what an old saved run means).
--   * backtest_indicators — a named custom-indicator definition (the node-graph DAG). Strategy
--     settings embed the definitions they use at save time, so runs stay reproducible.
-- Single-user: no owner scoping. Names are unique for a clean library list.

CREATE TABLE IF NOT EXISTS backtest_indicators (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    -- Node-graph definition: { nodes: [...], output: n }. Shape owned by backtest::custom.
    definition  JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS backtest_strategies (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    -- Full strategy configuration (engine `Settings` superset), incl. any embedded custom
    -- indicator defs. Fed straight to /run.
    settings    JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Provenance link on saved runs (nullable; SET NULL if the strategy is later deleted).
ALTER TABLE backtest_runs
    ADD COLUMN IF NOT EXISTS strategy_id UUID REFERENCES backtest_strategies(id) ON DELETE SET NULL;
