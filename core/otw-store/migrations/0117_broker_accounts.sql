-- Broker accounts: the account broker.
--
-- The data broker (`histdata_connectors`) answers "what did the market do"; this one
-- answers "what did *I* do": executions, positions, open orders, read-only. They are kept
-- apart on purpose. A data key and an account key are different credentials with different
-- blast radius, and granting a module the right to read prices is not granting it the right
-- to read a portfolio. Same shape as the other two brokers: a named account + sealed
-- credentials (plugged from the vault or local) + non-secret settings + module grants.

CREATE TABLE IF NOT EXISTS broker_accounts (
    id         UUID PRIMARY KEY,
    broker     TEXT NOT NULL,
    name       TEXT NOT NULL,
    config     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_broker_accounts_broker ON broker_accounts(broker);

-- One row per allowed module id; '*' means every account module, present and future.
CREATE TABLE IF NOT EXISTS broker_account_modules (
    account_id UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    module     TEXT NOT NULL,
    PRIMARY KEY (account_id, module)
);

-- Write-only credentials: a local sealed value, or a reference to a vault item.
CREATE TABLE IF NOT EXISTS broker_account_creds (
    id            UUID PRIMARY KEY,
    account_id    UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    broker        TEXT NOT NULL,
    name          TEXT NOT NULL,
    nonce         BYTEA,
    ciphertext    BYTEA,
    vault_item_id UUID REFERENCES vault_items(id),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, name)
);

-- An import batch now says where its rows came from: a file the user dropped, or a
-- broker account it pulled from.
ALTER TABLE journal_import_batches
    ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT 'file',
    ADD COLUMN IF NOT EXISTS broker_account_id UUID REFERENCES broker_accounts(id) ON DELETE SET NULL;

-- The last broker sync of a journal (a category is the book), so the modal reopens on the
-- account, the instruments and the window the user pulled last time instead of empty.
CREATE TABLE IF NOT EXISTS journal_broker_syncs (
    category_id    UUID PRIMARY KEY REFERENCES journal_categories(id) ON DELETE CASCADE,
    account_id     UUID REFERENCES broker_accounts(id) ON DELETE SET NULL,
    symbols        JSONB NOT NULL DEFAULT '[]'::jsonb,
    period_from    TIMESTAMPTZ,
    period_to      TIMESTAMPTZ,
    last_synced_at TIMESTAMPTZ,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
