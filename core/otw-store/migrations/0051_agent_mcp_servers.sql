-- External MCP servers the agent can connect OUT to (Streamable HTTP only — no local
-- process spawning by design). `auth_value` is the full header value (e.g. "Bearer sk-…"),
-- sealed at rest like provider API keys; empty = no auth. `catalog_id` is set when the
-- entry was added from the curated catalog (display only).
CREATE TABLE agent_mcp_servers (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    url         TEXT NOT NULL,
    auth_header TEXT NOT NULL DEFAULT 'Authorization',
    auth_value  TEXT NOT NULL DEFAULT '',
    catalog_id  TEXT NOT NULL DEFAULT '',
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Per-conversation selection of external servers (UUID array as JSONB; stale ids from
-- deleted servers are simply ignored at run time).
ALTER TABLE agent_conversations
    ADD COLUMN mcp_servers JSONB NOT NULL DEFAULT '[]'::jsonb;
