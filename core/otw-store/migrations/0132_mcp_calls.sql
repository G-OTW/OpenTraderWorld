-- MCP call log: one row per tool call reaching the agent gateway, from a remote client
-- (source 'mcp') or the in-app assistant (source 'agent'). It is what makes an agent's
-- error rate measurable: which endpoints fail, and why.
--
-- `route` is the catalog template when the call resolved to one, else the bare path asked
-- for. `status` is the HTTP status, 0 when the gateway refused the call before dispatch.
-- `class` names the failure (or, on a success, a note worth counting such as ignored query
-- params). Rows older than 30 days are pruned by the writer.

CREATE TABLE IF NOT EXISTS mcp_calls (
    id          BIGSERIAL PRIMARY KEY,
    at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    source      TEXT NOT NULL CHECK (source IN ('mcp', 'agent')),
    token_id    UUID REFERENCES mcp_tokens(id) ON DELETE SET NULL,
    token_name  TEXT NOT NULL,
    tool        TEXT NOT NULL,
    method      TEXT NOT NULL,
    route       TEXT NOT NULL,
    status      SMALLINT NOT NULL,
    ok          BOOLEAN NOT NULL,
    class       TEXT,
    detail      TEXT,
    duration_ms INTEGER NOT NULL,
    bytes       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS mcp_calls_at_idx ON mcp_calls (at DESC);
