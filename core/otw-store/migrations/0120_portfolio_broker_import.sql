-- Importing a broker account's holdings into a portfolio.
--
-- The batch gains the same two columns the journal's did: where the rows came from, and
-- which account answered. A batch is what a bad import is reverted by, so it has to say
-- more than a filename once the rows arrive over an API instead of in a file.
ALTER TABLE portfolio_import_batches
    ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT 'file',
    ADD COLUMN IF NOT EXISTS broker_account_id UUID REFERENCES broker_accounts(id) ON DELETE SET NULL;

-- The last broker sync of a portfolio, so the modal reopens on the account it was aligned
-- with last time instead of empty. The holdings themselves are never stored: they are read
-- from the broker each time, and a stale balance sheet presented as current is worse than
-- none.
CREATE TABLE IF NOT EXISTS portfolio_broker_syncs (
    portfolio_id   UUID PRIMARY KEY REFERENCES portfolios(id) ON DELETE CASCADE,
    account_id     UUID REFERENCES broker_accounts(id) ON DELETE SET NULL,
    last_synced_at TIMESTAMPTZ,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
