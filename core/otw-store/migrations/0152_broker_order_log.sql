-- Every order call sent to a broker and what the broker answered: the audit trail of chart
-- trading, and the source of the per-account order metrics shown in Settings.
--
-- Append-only: a trigger refuses UPDATE and DELETE, so a row once written stays as written.
-- No foreign key to broker_accounts on purpose: deleting an account must not rewrite (or be
-- blocked by) its history, so the account's id and name are copied onto each row.
CREATE TABLE broker_order_log (
    id          BIGSERIAL PRIMARY KEY,
    at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    account_id  UUID NOT NULL,
    account     TEXT NOT NULL,
    broker      TEXT NOT NULL,
    -- place | modify | cancel | cancel_all
    action      TEXT NOT NULL,
    request     JSONB NOT NULL,
    ok          BOOLEAN NOT NULL,
    -- The broker's answer: the placed ids, or the refusal as it was worded.
    response    JSONB NOT NULL,
    latency_ms  INTEGER NOT NULL
);

CREATE INDEX idx_broker_order_log_account_at ON broker_order_log (account_id, at DESC);
CREATE INDEX idx_broker_order_log_at ON broker_order_log (at DESC);

CREATE FUNCTION broker_order_log_append_only() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'broker_order_log is append-only';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER broker_order_log_no_rewrite
    BEFORE UPDATE OR DELETE ON broker_order_log
    FOR EACH ROW EXECUTE FUNCTION broker_order_log_append_only();
