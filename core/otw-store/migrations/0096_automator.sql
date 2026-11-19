-- Automator module: workflows, versions, schedules, runs.
--
-- A workflow is a DAG of nodes stored as JSONB. The engine walks it in topological order,
-- one node at a time, recording a row per step so a failed run says exactly which node
-- failed and with what payload. Nothing about the graph shape is a column: the schema
-- stores documents and the run history, the meaning lives in otw-core/src/automator/.
--
-- Permission envelope: `mcp_token_id` is the same idea as agent_agents.mcp_token_id. A
-- node calling an internal endpoint is checked against that token's per-module levels plus
-- the MCP catalog allowlist. No token on the workflow means no internal call may run
-- (fail closed).
--
-- Vault note: a node config may reference a vault item inside its JSONB (a header value,
-- an auth field). Those uses are NOT counted in vault.rs ITEM_USE_SQL (it would mean
-- scanning JSONB on every vault listing), so a vault item used only by a workflow shows as
-- unused. Deleting it breaks the node at run time, visibly, in the step error.

CREATE TABLE IF NOT EXISTS automator_workflows (
    id               UUID PRIMARY KEY,
    name             TEXT NOT NULL,
    description      TEXT NOT NULL DEFAULT '',
    -- { "nodes": [...], "edges": [...] }: the working copy, validated on write.
    graph            JSONB NOT NULL DEFAULT '{"nodes":[],"edges":[]}'::jsonb,
    -- Latest saved revision (mirrors automator_versions, same pattern as prompt_store).
    version_id       UUID,
    mcp_token_id     UUID REFERENCES mcp_tokens(id) ON DELETE SET NULL,
    favorite         BOOLEAN NOT NULL DEFAULT FALSE,
    -- A disabled workflow cannot be triggered by a schedule; a manual run still works.
    enabled          BOOLEAN NOT NULL DEFAULT TRUE,
    -- Watchdog deadline for one run, in seconds.
    max_runtime_secs INTEGER NOT NULL DEFAULT 300 CHECK (max_runtime_secs BETWEEN 10 AND 3600),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Immutable snapshots. Every save appends one; a rollback appends the old graph as a new
-- revision rather than rewriting history.
CREATE TABLE IF NOT EXISTS automator_versions (
    id          UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES automator_workflows(id) ON DELETE CASCADE,
    rev         INTEGER NOT NULL,
    graph       JSONB NOT NULL,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workflow_id, rev)
);

DO $$ BEGIN
    ALTER TABLE automator_workflows
        ADD CONSTRAINT automator_workflows_version_fk
        FOREIGN KEY (version_id) REFERENCES automator_versions(id) ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- Recurrence rules. The rule is stored in the user's timezone (IANA name) and next_run_at
-- is the resolved UTC instant, so the claim query stays a plain index scan.
CREATE TABLE IF NOT EXISTS automator_schedules (
    id            UUID PRIMARY KEY,
    workflow_id   UUID NOT NULL REFERENCES automator_workflows(id) ON DELETE CASCADE,
    -- NULL = always run the workflow's latest saved version. Set = pinned to that revision.
    version_id    UUID REFERENCES automator_versions(id) ON DELETE SET NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('interval', 'daily', 'weekly', 'monthly', 'once')),
    timezone      TEXT NOT NULL DEFAULT 'UTC',
    every_minutes INTEGER CHECK (every_minutes IS NULL OR every_minutes BETWEEN 1 AND 10080),
    at_hour       INTEGER CHECK (at_hour IS NULL OR at_hour BETWEEN 0 AND 23),
    at_minute     INTEGER CHECK (at_minute IS NULL OR at_minute BETWEEN 0 AND 59),
    -- Weekday bitmask, Monday = bit 0. NULL/0 on a weekly rule means every day.
    weekdays      SMALLINT,
    day_of_month  INTEGER CHECK (day_of_month IS NULL OR day_of_month BETWEEN 1 AND 31),
    run_at        TIMESTAMPTZ,
    -- On boot, fire once if occurrences were missed while the app was down. Several missed
    -- occurrences coalesce into a single catch-up run.
    catch_up      BOOLEAN NOT NULL DEFAULT FALSE,
    active        BOOLEAN NOT NULL DEFAULT TRUE,
    next_run_at   TIMESTAMPTZ,
    last_run_at   TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_automator_sched_due
    ON automator_schedules(next_run_at) WHERE active;
CREATE INDEX IF NOT EXISTS idx_automator_sched_wf
    ON automator_schedules(workflow_id);

CREATE TABLE IF NOT EXISTS automator_runs (
    id               UUID PRIMARY KEY,
    workflow_id      UUID NOT NULL REFERENCES automator_workflows(id) ON DELETE CASCADE,
    version_id       UUID REFERENCES automator_versions(id) ON DELETE SET NULL,
    schedule_id      UUID REFERENCES automator_schedules(id) ON DELETE SET NULL,
    trigger          TEXT NOT NULL CHECK (trigger IN ('manual', 'schedule', 'catchup', 'test')),
    -- 'test' runs simulate writes and sends; they are kept out of the live retention count.
    mode             TEXT NOT NULL DEFAULT 'live' CHECK (mode IN ('live', 'test')),
    status           TEXT NOT NULL DEFAULT 'queued'
                     CHECK (status IN ('queued', 'running', 'ok', 'failed', 'cancelled', 'timeout')),
    cancel_requested BOOLEAN NOT NULL DEFAULT FALSE,
    -- Manual run parameters, readable from a node as {{input.<key>}}.
    input            JSONB NOT NULL DEFAULT '{}'::jsonb,
    error            TEXT NOT NULL DEFAULT '',
    started_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at      TIMESTAMPTZ,
    duration_ms      INTEGER
);
CREATE INDEX IF NOT EXISTS idx_automator_runs_wf
    ON automator_runs(workflow_id, started_at DESC);
-- Drives the "already running?" guard of the claim query.
CREATE INDEX IF NOT EXISTS idx_automator_runs_live
    ON automator_runs(workflow_id) WHERE status IN ('queued', 'running');

CREATE TABLE IF NOT EXISTS automator_run_steps (
    id          UUID PRIMARY KEY,
    run_id      UUID NOT NULL REFERENCES automator_runs(id) ON DELETE CASCADE,
    node_id     TEXT NOT NULL,
    node_name   TEXT NOT NULL DEFAULT '',
    kind        TEXT NOT NULL,
    seq         INTEGER NOT NULL,
    status      TEXT NOT NULL CHECK (status IN ('ok', 'failed', 'skipped', 'cancelled', 'simulated')),
    -- What the node was about to do, after expression resolution and secret redaction.
    request     JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- The node's output, or a blob handle when it exceeded the inline cap.
    output      JSONB NOT NULL DEFAULT 'null'::jsonb,
    error       TEXT NOT NULL DEFAULT '',
    bytes       INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_automator_steps_run
    ON automator_run_steps(run_id, seq);

-- Payloads too large to inline in a step output (a downloaded CSV, a big JSON response).
-- A node passes the handle downstream, so a megabyte never lands inside a prompt by
-- accident. Deleted with the run.
CREATE TABLE IF NOT EXISTS automator_blobs (
    id           UUID PRIMARY KEY,
    run_id       UUID NOT NULL REFERENCES automator_runs(id) ON DELETE CASCADE,
    node_id      TEXT NOT NULL DEFAULT '',
    content_type TEXT NOT NULL DEFAULT 'application/octet-stream',
    bytes        INTEGER NOT NULL DEFAULT 0,
    data         BYTEA NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_automator_blobs_run
    ON automator_blobs(run_id);
