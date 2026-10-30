-- Strategies on a broker account: a paper session whose fills happen at a broker (the paper
-- broker account first, then a live one). The engine decides, the broker executes, and a sync
-- converges the book the engine intends with the one the account holds.
--
-- `execution` is the target, set at creation: NULL (or target `simulate`) is today's session,
-- unchanged. `exec_account_id` repeats its account so a disarm and the guard below are plain
-- indexed queries. A session may send orders only while armed (`armed_at`, `armed_by`): arming
-- replaces the browser's trading unlock, and every disarm (background trading off, trading off,
-- the trading lock turned on, a kill switch) clears it so the user re-arms knowingly.
--
-- `exec_state` is what the last sync found: `idle` (not armed), `synced`, `degraded` (the
-- account holds what this strategy's fills do not explain: no new exposure, exits still
-- managed), `paused` (a halt on the account: only orders that take risk off go out),
-- `blocked` (an order's answer was lost: nothing on that instrument until it is settled),
-- `error` (the account could not be read). `exec_note` says why, `exec_book` is the last
-- intended and broker book as the dashboard shows it, and `exec_baseline` what the account
-- already held of each instrument when the session was armed (a spot account holds the asset
-- it trades), so a drift is measured against the strategy's own fills only.
ALTER TABLE paper_sessions
    ADD COLUMN execution       JSONB,
    ADD COLUMN exec_account_id UUID REFERENCES broker_accounts(id) ON DELETE SET NULL,
    ADD COLUMN armed_at        TIMESTAMPTZ,
    ADD COLUMN armed_by        UUID,
    ADD COLUMN exec_state      TEXT NOT NULL DEFAULT 'idle',
    ADD COLUMN exec_note       TEXT NOT NULL DEFAULT '',
    ADD COLUMN exec_synced_at  TIMESTAMPTZ,
    ADD COLUMN exec_book       JSONB,
    ADD COLUMN exec_baseline   JSONB NOT NULL DEFAULT '{}';

ALTER TABLE paper_sessions ADD CONSTRAINT paper_sessions_exec_state
    CHECK (exec_state IN ('idle', 'synced', 'degraded', 'paused', 'blocked', 'error'));
ALTER TABLE paper_sessions ADD CONSTRAINT paper_sessions_armed_needs_execution
    CHECK (armed_at IS NULL OR execution IS NOT NULL);

CREATE INDEX idx_paper_sessions_exec_account ON paper_sessions (exec_account_id)
    WHERE exec_account_id IS NOT NULL;

-- One account, one instrument, one session: two strategies on one position would fight over
-- it. A row per instrument a broker session executes, by the broker's own spelling.
CREATE TABLE paper_exec_instruments (
    account_id UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    symbol     TEXT NOT NULL,
    session_id UUID NOT NULL REFERENCES paper_sessions(id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, symbol)
);

CREATE INDEX idx_paper_exec_instruments_session ON paper_exec_instruments (session_id);

-- Every order a session decided to send, one row per leg, written before the call leaves.
-- `intent` names the decision (an entry at a close, a resting limit, the exits of a position,
-- a close): one row per intent per session, ever, so a decision is never sent twice, across
-- restarts. `outcome` is what the sender answered (`sending` until it answers); `status` and
-- `filled_qty` are what the broker last said of it.
CREATE TABLE paper_exec_orders (
    session_id   UUID NOT NULL REFERENCES paper_sessions(id) ON DELETE CASCADE,
    account_id   UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    client_id    TEXT NOT NULL,
    intent       TEXT NOT NULL,
    role         TEXT NOT NULL CHECK (role IN ('entry', 'limit', 'stop', 'target', 'close')),
    ticker       TEXT NOT NULL,
    symbol       TEXT NOT NULL,
    side         TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    qty          NUMERIC NOT NULL,
    limit_price  NUMERIC,
    stop_price   NUMERIC,
    -- What the engine decided, as it decided it: its size, and its price (the close for a
    -- market order, the level for a limit, a stop or a target). Shown next to what was sent.
    engine_qty   DOUBLE PRECISION,
    engine_price DOUBLE PRECISION,
    -- The entry this order belongs to (its client id), empty for an entry itself.
    position     TEXT NOT NULL DEFAULT '',
    outcome      TEXT NOT NULL CHECK (outcome IN ('sending', 'sent', 'refused', 'unknown')),
    reason       TEXT,
    venue_id     TEXT,
    -- Where the broker says the order stands, looked up by its client id (NULL until the
    -- first lookup), and how much of it filled: the session's position is the sum of these.
    status       TEXT CHECK (status IN ('working', 'filled', 'cancelled', 'rejected', 'expired')),
    filled_qty   NUMERIC NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, client_id),
    UNIQUE (session_id, intent)
);

CREATE INDEX idx_paper_exec_orders_session ON paper_exec_orders (session_id, created_at);

-- The fills the broker reported for a session's orders, keyed by the broker's execution id
-- (a fill read twice is one fill). Decimal as the venue states it; the simulated price the
-- engine gave the same trade is read beside it on the dashboard.
CREATE TABLE paper_exec_fills (
    session_id   UUID NOT NULL REFERENCES paper_sessions(id) ON DELETE CASCADE,
    account_id   UUID NOT NULL,
    exec_id      TEXT NOT NULL,
    client_id    TEXT NOT NULL,
    symbol       TEXT NOT NULL,
    side         TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    qty          NUMERIC NOT NULL,
    price        NUMERIC NOT NULL,
    fee          NUMERIC NOT NULL DEFAULT 0,
    fee_currency TEXT NOT NULL DEFAULT '',
    at           TIMESTAMPTZ NOT NULL,
    recorded_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (session_id, exec_id)
);

CREATE INDEX idx_paper_exec_fills_session ON paper_exec_fills (session_id, at);
