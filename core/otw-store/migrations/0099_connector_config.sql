-- Non-secret connector settings.
--
-- A credential is sealed (histdata_provider_creds, optionally a vault reference); a
-- *setting* is not a secret and must stay readable: an IB Gateway host and port are
-- addresses, not keys, and hiding them behind the vault's write-only shape would make a
-- connection impossible to diagnose. They live here in the clear, one JSON object per
-- connector, keyed by the field names the provider declares in its capability.
ALTER TABLE histdata_connectors
    ADD COLUMN IF NOT EXISTS config JSONB NOT NULL DEFAULT '{}'::jsonb;
