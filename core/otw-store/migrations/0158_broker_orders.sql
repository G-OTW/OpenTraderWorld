-- Every order leg OpenTraderWorld sent to a broker, keyed by its client id, and where it
-- stands as far as OTW knows: the state that survives a restart (onboarding spec 6.1).
--
-- A leg is written `pending` before the call leaves, so a client id is sent at most once per
-- account, ever. The answer moves it to `working` (the broker's id stored) or `rejected`; no
-- answer and no lookup leaves it `unknown`, which blocks new orders on its instrument until a
-- lookup by client id settles it. A process that dies mid-call leaves `pending`, read back as
-- `unknown` on the next start. Final states (filled, cancelled, rejected, expired) are the
-- broker's, read back by the reconciliation cycle.
--
-- Unlike `broker_order_log` this is a state table, not a trail: rows are updated in place,
-- and every change is also written to the log.
CREATE TABLE broker_orders (
    account_id  UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    client_id   TEXT NOT NULL,
    broker      TEXT NOT NULL,
    -- The instrument as the ticket named it (the chart's ticker, or the position id a close
    -- was sent on), and the chart's asset type (empty for a close).
    ticker      TEXT NOT NULL,
    asset_type  TEXT NOT NULL,
    -- place | close
    action      TEXT NOT NULL,
    -- The broker's id, once it accepted the leg.
    venue_id    TEXT,
    -- The client id of the entry this exit hangs on, and the one-cancels-other group.
    parent      TEXT,
    grp         TEXT,
    status      TEXT NOT NULL CHECK (status IN ('pending', 'unknown', 'working', 'filled', 'cancelled', 'rejected', 'expired')),
    filled_qty  NUMERIC,
    reason      TEXT,
    -- Lookups answered "never seen" in a row: two settle an unknown leg as not received.
    not_found   INTEGER NOT NULL DEFAULT 0,
    -- The user was told this leg's outcome could not be settled.
    alerted     BOOLEAN NOT NULL DEFAULT FALSE,
    request     JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, client_id)
);

CREATE INDEX idx_broker_orders_open ON broker_orders (account_id, ticker)
    WHERE status IN ('pending', 'unknown', 'working');

-- The positions each trading account held at its last reconciliation, by the broker's own
-- key for them: a change between two reads that its fills do not explain is drift.
CREATE TABLE broker_positions_seen (
    account_id  UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    position    TEXT NOT NULL,
    symbol      TEXT NOT NULL,
    -- Signed: long positive, short negative.
    qty         NUMERIC NOT NULL,
    seen_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, position)
);
