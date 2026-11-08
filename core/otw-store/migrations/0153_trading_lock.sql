-- Trading lock: while on, an order sent from a session is refused until that session has
-- typed a current authenticator code, once per session for every broker account.
-- It needs the second factor, so removing the factor (Settings or the host CLI) turns the
-- lock off and strips the trading grant from every broker account in the same step.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS trading_lock BOOLEAN NOT NULL DEFAULT FALSE;
