-- OAuth 2.1 for the MCP gateway. A client registers itself (RFC 7591), the owner approves
-- it on a consent page, and the approval becomes an ordinary `mcp_tokens` row (the grant)
-- carrying the permissions chosen there. Access and refresh tokens are short-lived
-- children of that grant: revoking the grant in Settings kills both.
CREATE TABLE mcp_oauth_clients (
    id            TEXT PRIMARY KEY,               -- client_id handed to the client
    name          TEXT NOT NULL,
    redirect_uris TEXT[] NOT NULL,
    secret_hash   TEXT,                           -- SHA-256 of the client secret; NULL = public client
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE mcp_tokens
    ADD COLUMN oauth_client_id TEXT REFERENCES mcp_oauth_clients(id) ON DELETE CASCADE;

-- Authorization codes: single use, two minutes. The grant is only created on exchange, so
-- an approval nobody redeems leaves nothing behind.
CREATE TABLE mcp_oauth_codes (
    code_hash      TEXT PRIMARY KEY,
    client_id      TEXT NOT NULL REFERENCES mcp_oauth_clients(id) ON DELETE CASCADE,
    redirect_uri   TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    resource       TEXT,
    name           TEXT NOT NULL,
    permissions    JSONB NOT NULL,
    grant_expires_at TIMESTAMPTZ,
    expires_at     TIMESTAMPTZ NOT NULL
);

CREATE TABLE mcp_oauth_tokens (
    token_hash TEXT PRIMARY KEY,
    grant_id   UUID NOT NULL REFERENCES mcp_tokens(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('access', 'refresh')),
    expires_at TIMESTAMPTZ NOT NULL,
    used_at    TIMESTAMPTZ,                       -- refresh only: set when rotated
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX mcp_oauth_tokens_grant ON mcp_oauth_tokens (grant_id);
