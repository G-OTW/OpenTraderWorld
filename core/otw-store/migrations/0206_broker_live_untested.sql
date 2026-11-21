-- E2D (onboarding spec 9, plan 2026-11-11): live orders on a venue whose order path was built
-- from its documentation and never run against it.
--
-- broker_live_untested: the account owner's acknowledgement that the broker was never tested
-- against the venue, signed when live trading was turned on; dropped when it is turned off.
CREATE TABLE broker_live_untested (
    account_id      UUID PRIMARY KEY REFERENCES broker_accounts(id) ON DELETE CASCADE,
    statement       TEXT NOT NULL,
    acknowledged_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
