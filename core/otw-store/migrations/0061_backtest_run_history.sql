-- Backtest run history.
--
-- Until now only *explicitly saved* runs were persisted, and prune_runs() capped the table at
-- 20 rows — which silently evicted named runs too. Two changes:
--   * every run (UI or MCP agent) is now recorded automatically under a generated name, so the
--     result of a run is never lost the moment the HTTP response is gone;
--   * `pinned` separates the two levels: false = auto history (capped, sweepable), true = the
--     user named it via Save (never pruned, never touched by "delete all").
-- Existing rows were all deliberately saved, so they become pinned.

ALTER TABLE backtest_runs
    ADD COLUMN IF NOT EXISTS pinned BOOL NOT NULL DEFAULT false;

UPDATE backtest_runs SET pinned = true;

-- History reads filter on pinned and order by recency.
CREATE INDEX IF NOT EXISTS idx_backtest_runs_pinned_created
    ON backtest_runs(pinned, created_at DESC);
