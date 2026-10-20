-- Webhooks module.
--
-- An endpoint is an inbound URL (`/api/hooks/{token}`) that redirects each received
-- payload to an existing module (its `target`). Sender-agnostic: many alerting services
-- can only POST to a bare URL (no custom headers), so the credential is the high-entropy
-- token in the URL path; like MCP tokens, only its SHA-256 is stored and the plaintext is
-- shown once at creation. `webhook_events` keeps a short per-endpoint delivery log for
-- debugging sender setups.

CREATE TABLE IF NOT EXISTS webhook_endpoints (
    id               UUID PRIMARY KEY,
    name             TEXT NOT NULL,
    token_hash       TEXT NOT NULL UNIQUE,
    -- First characters of the plaintext token, for display ("whk_ab12…").
    prefix           TEXT NOT NULL,
    -- Module the payload is redirected to; must match the dispatch registry in otw-core.
    target           TEXT NOT NULL DEFAULT 'remindme',
    -- Per-target options (reserved; empty in v1).
    config           JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled          BOOLEAN NOT NULL DEFAULT TRUE,
    received_count   BIGINT NOT NULL DEFAULT 0,
    last_received_at TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Per-endpoint delivery log, trimmed to the most recent entries on insert.
CREATE TABLE IF NOT EXISTS webhook_events (
    id          UUID PRIMARY KEY,
    endpoint_id UUID NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    -- 'ok' | 'error' | 'ignored' (endpoint disabled).
    status      TEXT NOT NULL,
    detail      TEXT NOT NULL DEFAULT '',
    -- Received body, truncated (see store) — for debugging alert message setups.
    payload     TEXT NOT NULL DEFAULT '',
    received_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_webhook_events_endpoint
    ON webhook_events(endpoint_id, received_at DESC);
