-- Backtest v2: multi-asset + engine versioning.
--
-- Two additions to saved runs:
--   * engine_version — the semantics version the run was produced under (see backtest::
--     ENGINE_VERSION). Old rows predate stamping, so default to 1 (the engine at that time).
--   * dataset_ids — the full set of datasets a portfolio run spanned. The legacy scalar
--     dataset_id is kept (single-asset runs + FK ON DELETE SET NULL for browsability); for a
--     multi-asset run dataset_id holds the first/primary dataset and dataset_ids the whole set.
-- Both are additive and nullable/defaulted, so every v1 saved run still lists and reruns.

ALTER TABLE backtest_runs
    ADD COLUMN IF NOT EXISTS engine_version INT NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS dataset_ids UUID[];
