-- Script indicators (Pine v5/v6 compatible source) next to the node-graph DAG rows, and
-- per-instrument facts a script may read (`syminfo.*`).
--
--   * backtest_indicators.kind — 'dag' (definition = node graph, as before) | 'script'.
--     A script row stores its `source` and `meta` = {"script": ScriptMeta, "warnings":
--     [Diagnostic]} written by the server's compile, never by the client.
--   * instrument_meta — tick size, point value, exchange timezone, session, … keyed by
--     instrument (not dataset: the chart draws instruments that are not stored). NULL =
--     unknown: a script reading it is refused, never defaulted.

ALTER TABLE backtest_indicators
    ADD COLUMN IF NOT EXISTS kind   TEXT NOT NULL DEFAULT 'dag',
    ADD COLUMN IF NOT EXISTS source TEXT,
    ADD COLUMN IF NOT EXISTS meta   JSONB,
    ALTER COLUMN definition DROP NOT NULL;

ALTER TABLE backtest_indicators ADD CONSTRAINT backtest_indicators_kind_chk CHECK (
    (kind = 'dag' AND definition IS NOT NULL)
    OR (kind = 'script' AND source IS NOT NULL AND meta IS NOT NULL));

CREATE TABLE IF NOT EXISTS instrument_meta (
    provider     TEXT NOT NULL,
    asset_type   TEXT NOT NULL,
    ticker       TEXT NOT NULL,
    mintick      DOUBLE PRECISION,
    pointvalue   DOUBLE PRECISION,
    timezone     TEXT,
    session      TEXT,
    kind         TEXT,
    currency     TEXT,
    basecurrency TEXT,
    description  TEXT,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, asset_type, ticker)
);
