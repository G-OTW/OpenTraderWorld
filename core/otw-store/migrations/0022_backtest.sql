-- Backtest module.
--
-- Runs a strategy over stored histdata bars. The simulation itself is stateless (POST the
-- settings, get back trades + stats + equity) — only *saved* runs are persisted here, so a
-- user can rerun a configuration later and browse a history of results. We store the full
-- settings (to rerun) plus a snapshot of the summary stats only (not every trade/equity
-- point), keeping the table small; the trades/equity are recomputed on demand.
-- Single-user: no owner scoping.

CREATE TABLE IF NOT EXISTS backtest_runs (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    -- The dataset the run was built on. SET NULL if the dataset is later deleted so the
    -- saved settings/stats stay browsable (rerun will then fail with a clear message).
    dataset_id  UUID REFERENCES histdata_datasets(id) ON DELETE SET NULL,
    -- Denormalized coordinates so history reads without joining a (possibly gone) dataset.
    ticker      TEXT NOT NULL,
    timeframe   TEXT NOT NULL,
    -- Full strategy configuration, shape owned by the engine (signal, entry, sl/tp, fees,
    -- sizing, leverage, spread). Rerun feeds this straight back to /run.
    settings    JSONB NOT NULL,
    -- Summary stats snapshot only (net pnl, return %, win rate, trade count, max drawdown,
    -- profit factor, …). Not the per-trade list — that is recomputed on rerun.
    stats       JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_backtest_runs_created ON backtest_runs(created_at DESC);
