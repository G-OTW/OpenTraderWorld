-- Date-windowed backtest runs.
--
-- A run could only ever span a whole dataset (capped by a bar `limit`), which made two things
-- inexpressible: walk-forward (roll a window forward through the history) and regime slices
-- (split the history and compare each part). Both are required by the anti-overfitting
-- protocol, so `/api/backtest/run` now takes `from`/`to`.
--
-- The window has to be persisted, not just applied: replay paths — the chart rerun and the
-- Monte-Carlo route — rebuild a run from its stored settings + dataset ids alone. Without
-- these columns a windowed run would replay over the full history and silently resample a
-- different trade list than the one it reports.
--
-- NULL on both = the whole dataset, which is every pre-existing row and every unwindowed run.
ALTER TABLE backtest_runs
    ADD COLUMN IF NOT EXISTS bars_from TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS bars_to   TIMESTAMPTZ;
