-- Scheduled broker syncs, the fills they refuse to guess, and one run log for every
-- background job.

-- A journal's broker sync becomes replayable: everything the modal asked is kept, plus
-- the answers the user gave to past conflicts.
--   resume_from  where the next scheduled pull starts: the opening fill of the oldest
--                position still open, so the fold always sees a position from its start.
--   seeds        positions opened before any pull, as the user described them
--                ([{id, symbol, side, qty, price, entry_at, cutoff, currency, asset_class,
--                multiplier}]). Each one stands in for the history the broker never sent.
--   trusted      symbols whose fills are taken as they fold, without the positions check.
--   excluded     symbols the sync leaves alone.
-- `auto_enabled` is whether the schedule exists, `auto_paused` whether it runs.
ALTER TABLE journal_broker_syncs
    ADD COLUMN IF NOT EXISTS auto_enabled     BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS auto_paused      BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS interval_minutes INTEGER NOT NULL DEFAULT 60,
    ADD COLUMN IF NOT EXISTS allow_short      BOOLEAN,
    ADD COLUMN IF NOT EXISTS strategy_id      UUID REFERENCES journal_strategies(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS template_id      UUID REFERENCES journal_templates(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS resume_from      TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS seeds            JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS trusted          JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS excluded         JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS last_run_at      TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS last_error       TEXT;

ALTER TABLE portfolio_broker_syncs
    ADD COLUMN IF NOT EXISTS auto_enabled     BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS auto_paused      BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS interval_minutes INTEGER NOT NULL DEFAULT 1440,
    ADD COLUMN IF NOT EXISTS excluded         JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS last_run_at      TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS last_error       TEXT;

-- What a sync could not decide on its own, one row per instrument and book. The rest of
-- the sync goes through; this instrument waits for the user's answer.
--   module     journal | portfolios
--   target_id  the journal category or the portfolio (no FK: two tables)
--   kind       journal: unexplained_position | sell_without_open
--              portfolios: unresolved | no_price
CREATE TABLE IF NOT EXISTS broker_sync_conflicts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    module      TEXT NOT NULL,
    target_id   UUID NOT NULL,
    account_id  UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    symbol      TEXT NOT NULL,
    kind        TEXT NOT NULL,
    details     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (module, target_id, account_id, symbol)
);

-- Pause switch for the jobs the app ships with (FX catch-up, managers' portfolios). Their
-- cadence is fixed in code; only whether they run is the user's.
CREATE TABLE IF NOT EXISTS system_jobs (
    id          TEXT PRIMARY KEY,
    enabled     BOOLEAN NOT NULL DEFAULT true,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One row per run of any background job, keyed by the job's id
-- (`journal-sync:<category>`, `portfolio-sync:<portfolio>`, `fx`, `mportfolios`).
CREATE TABLE IF NOT EXISTS job_runs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id      TEXT NOT NULL,
    trigger     TEXT NOT NULL,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    status      TEXT NOT NULL DEFAULT 'running',
    summary     TEXT,
    error       TEXT
);
CREATE INDEX IF NOT EXISTS job_runs_job_started ON job_runs (job_id, started_at DESC);
