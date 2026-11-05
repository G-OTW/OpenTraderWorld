-- MCP (Model Context Protocol) access tokens.
--
-- AI agents authenticate to POST /api/mcp with a bearer token created from
-- Settings → MCP. Only the SHA-256 of the token is stored — the plaintext is shown
-- once at creation and never persisted. `prefix` keeps the first characters so the
-- UI can identify a token without revealing it.
--
-- `permissions` maps module → access level ({"journal": "rw", "histdata": "r"}).
-- Modules absent from the map are inaccessible for that token. The MCP endpoint
-- itself is additionally gated by the global `mcp_enabled` app setting (default off).

CREATE TABLE IF NOT EXISTS mcp_tokens (
    id           UUID PRIMARY KEY,
    name         TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,          -- SHA-256 hex of the full bearer token
    prefix       TEXT NOT NULL,                 -- display-only leading characters
    permissions  JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);
