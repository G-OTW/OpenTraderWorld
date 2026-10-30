-- Trade on alert: a chart alert that, when it fires, sends a ticket to one broker account.
--
-- One row per alert at most, deleted with it. The ticket is a template (`TicketTemplate` in
-- otw-core's `alert_trade`): sizes and prices relative to the price that fires the alert,
-- materialized into an order at fire time. An alert with a trade fires once.
--
-- `armed_at` / `armed_by` are the standing authorization a background send needs on top of
-- the account's background trading permission: set by the user (with the authenticator code
-- when the trading lock is on), cleared by any edit of the account, the ticket or the
-- fallback, by every disarm hook, and atomically by the fire that takes it. A row is
-- disarmed before its order leaves, so a restart never sends it twice.
--
-- `status` says how the last fire went:
--   idle            never fired since it was written or armed
--   sending         taken by a fire, the order on its way (a restart leaves it unknown)
--   sent            placed at the broker, not filled yet
--   filled          filled (in part or whole)
--   cancelled       ended at the broker without a fill
--   refused         refused before or by the broker, no fallback asked
--   unknown         the broker's answer was lost: check at the broker
--   fallback_done   refused, and the fallback ran
--   fallback_failed refused or unknown, and the fallback could not finish
CREATE TABLE alert_trades (
    alert_id    UUID PRIMARY KEY REFERENCES histviz_alerts(id) ON DELETE CASCADE,
    account_id  UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    intent      TEXT NOT NULL CHECK (intent IN ('enter', 'exit')),
    ticket      JSONB NOT NULL,
    fallback    TEXT NOT NULL DEFAULT 'none'
                CHECK (fallback IN ('none', 'cancel_symbol', 'close_symbol', 'flatten_account')),
    armed_at    TIMESTAMPTZ,
    armed_by    UUID,
    status      TEXT NOT NULL DEFAULT 'idle'
                CHECK (status IN ('idle', 'sending', 'sent', 'filled', 'cancelled', 'refused',
                                  'unknown', 'fallback_done', 'fallback_failed')),
    -- Placed ids, the materialized ticket, the refusal code and sentence, the fallback outcome.
    last_result JSONB NOT NULL DEFAULT '{}',
    fired_at    TIMESTAMPTZ,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((armed_at IS NULL) = (armed_by IS NULL))
);

CREATE INDEX idx_alert_trades_armed ON alert_trades (account_id) WHERE armed_at IS NOT NULL;
CREATE INDEX idx_alert_trades_followed ON alert_trades (status) WHERE status IN ('sending', 'sent', 'unknown');
