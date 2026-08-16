-- Centralized API request quotas + named histdata connectors.
--
-- `api_quotas` is one shared usage counter per tracked scope. A scope is a string key
-- owned by the module that records into it: `feed:<uuid>` (news API sources) and
-- `histconn:<uuid>` (historical-data connectors) for now. The window rolls on
-- date_trunc(period, now()): a bump/read outside the stored window resets `used`.
-- Observe-and-display only — nothing throttles on it (same philosophy as api_rate).
CREATE TABLE IF NOT EXISTS api_quotas (
    scope        TEXT PRIMARY KEY,
    max_requests BIGINT,                          -- NULL = unlimited (tracked, no cap)
    period       TEXT NOT NULL DEFAULT 'day',     -- minute | hour | day | week | month
    period_start TIMESTAMPTZ NOT NULL DEFAULT now(),
    used         BIGINT NOT NULL DEFAULT 0,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Named provider connectors for the Historical Data module. Several connectors may share
-- one provider (e.g. two Alpha Vantage keys); each carries its own credentials and its own
-- optional request quota (scope `histconn:<id>`). One default connector per known provider
-- is seeded below so existing installs keep working unchanged.
CREATE TABLE IF NOT EXISTS histdata_connectors (
    id         UUID PRIMARY KEY,
    provider   TEXT NOT NULL,
    name       TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_histdata_connectors_provider ON histdata_connectors(provider);

INSERT INTO histdata_connectors (id, provider, name) VALUES
    (gen_random_uuid(), 'binance',      'Binance'),
    (gen_random_uuid(), 'coinbase',     'Coinbase'),
    (gen_random_uuid(), 'kraken',       'Kraken'),
    (gen_random_uuid(), 'alphavantage', 'Alpha Vantage'),
    (gen_random_uuid(), 'eodhd',        'EODHD'),
    (gen_random_uuid(), 'yahoo',        'Yahoo Finance'),
    (gen_random_uuid(), 'alpaca',       'Alpaca'),
    (gen_random_uuid(), 'massive',      'Massive (Polygon.io)')
ON CONFLICT (name) DO NOTHING;

-- Credentials move from provider-wide to per-connector. Existing rows attach to the
-- seeded default connector of their provider; uniqueness becomes (connector_id, name)
-- so two connectors of the same provider can each hold an `api_key`.
ALTER TABLE histdata_provider_creds
    ADD COLUMN IF NOT EXISTS connector_id UUID REFERENCES histdata_connectors(id) ON DELETE CASCADE;
UPDATE histdata_provider_creds c
    SET connector_id = k.id
    FROM histdata_connectors k
    WHERE c.connector_id IS NULL AND k.provider = c.provider;
-- Orphans (creds of a provider we no longer ship a connector for) would break NOT NULL.
DELETE FROM histdata_provider_creds WHERE connector_id IS NULL;
ALTER TABLE histdata_provider_creds ALTER COLUMN connector_id SET NOT NULL;
ALTER TABLE histdata_provider_creds DROP CONSTRAINT IF EXISTS histdata_provider_creds_provider_name_key;
ALTER TABLE histdata_provider_creds
    ADD CONSTRAINT histdata_provider_creds_connector_name_key UNIQUE (connector_id, name);

-- Jobs remember which connector queued them so the worker uses that connector's
-- credentials and bills its quota. NULL (pre-migration jobs) falls back to the
-- provider's default connector.
ALTER TABLE histdata_jobs
    ADD COLUMN IF NOT EXISTS connector_id UUID REFERENCES histdata_connectors(id) ON DELETE SET NULL;
