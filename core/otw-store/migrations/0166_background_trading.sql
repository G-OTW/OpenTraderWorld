-- Background trading: the permission, signed once per broker account, for orders sent while
-- nobody is watching (a trade on alert, a strategy on a broker). The row is the record of
-- what was shown and accepted, when, in which language and with which second factor; the
-- column on the account points at the signature in force, NULL being off.
--
-- Append-only, like broker_order_log: turning the permission off clears the account's column
-- and keeps the row. The one change the trigger lets through is the account's deletion
-- detaching its rows (ON DELETE SET NULL), so the account's name, broker and environment are
-- copied onto each row and the record still reads whole once the account is gone.
CREATE TABLE trading_acknowledgments (
    id            BIGSERIAL PRIMARY KEY,
    user_id       UUID NOT NULL,
    account_id    UUID REFERENCES broker_accounts(id) ON DELETE SET NULL,
    account_name  TEXT NOT NULL,
    broker        TEXT NOT NULL,
    environment   TEXT NOT NULL CHECK (environment IN ('paper', 'sandbox', 'live')),
    -- The terms' update date shown at signature, and the terms sections as they were shown.
    terms_version TEXT NOT NULL,
    terms         JSONB NOT NULL,
    -- The exact sentences ticked, in the language shown: [{ key, text }].
    statements    JSONB NOT NULL,
    locale        TEXT NOT NULL,
    -- The account name as the user typed it.
    typed         TEXT NOT NULL,
    -- An authenticator code was verified in the same request.
    second_factor BOOLEAN NOT NULL,
    signed_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_trading_acknowledgments_account ON trading_acknowledgments (account_id, signed_at DESC);

CREATE FUNCTION trading_acknowledgments_append_only() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND OLD.account_id IS NOT NULL AND NEW.account_id IS NULL
       AND (to_jsonb(NEW) - 'account_id') = (to_jsonb(OLD) - 'account_id') THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'trading_acknowledgments is append-only';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trading_acknowledgments_no_rewrite
    BEFORE UPDATE OR DELETE ON trading_acknowledgments
    FOR EACH ROW EXECUTE FUNCTION trading_acknowledgments_append_only();

ALTER TABLE broker_accounts ADD COLUMN background_trading_ack BIGINT REFERENCES trading_acknowledgments(id);
