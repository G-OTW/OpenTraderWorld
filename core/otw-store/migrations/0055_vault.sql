-- Centralized secrets vault.
--
-- A vault groups the credentials of one external service (e.g. "Binance"); each item is
-- one named sealed value (`apikey`, `secretkey`, ...). Values are write-only: sealed with
-- the shared XChaCha20-Poly1305 cipher on save, decrypted only inside outbound call paths,
-- never returned over the API. Request tracking / rate limits reuse `api_quotas` with a
-- `vault:<uuid>` scope — usage is counted per vault, not per item.
--
-- Existing secret stores (feed_secrets, histdata_provider_creds, agent_providers,
-- agent_mcp_servers, notif_channels) gain an optional reference to a vault item; when set
-- the value resolves from the vault and any locally sealed copy is ignored. Legacy local
-- values keep working untouched.

CREATE TABLE IF NOT EXISTS vaults (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS vaults_name_uniq ON vaults (lower(name));

CREATE TABLE IF NOT EXISTS vault_items (
    id         UUID PRIMARY KEY,
    vault_id   UUID NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    nonce      BYTEA NOT NULL,
    ciphertext BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (vault_id, name)
);

-- Consumer references. Default FK action (NO ACTION) is intentional: deleting a vault
-- item that is still plugged somewhere must fail; the API pre-checks and returns 409.
ALTER TABLE feed_secrets
    ALTER COLUMN nonce DROP NOT NULL,
    ALTER COLUMN ciphertext DROP NOT NULL,
    ADD COLUMN IF NOT EXISTS vault_item_id UUID REFERENCES vault_items(id);

ALTER TABLE histdata_provider_creds
    ALTER COLUMN nonce DROP NOT NULL,
    ALTER COLUMN ciphertext DROP NOT NULL,
    ADD COLUMN IF NOT EXISTS vault_item_id UUID REFERENCES vault_items(id);

ALTER TABLE agent_providers
    ADD COLUMN IF NOT EXISTS api_key_vault_item UUID REFERENCES vault_items(id);

ALTER TABLE agent_mcp_servers
    ADD COLUMN IF NOT EXISTS auth_vault_item UUID REFERENCES vault_items(id);

ALTER TABLE notif_channels
    ADD COLUMN IF NOT EXISTS secret_vault_item UUID REFERENCES vault_items(id);
