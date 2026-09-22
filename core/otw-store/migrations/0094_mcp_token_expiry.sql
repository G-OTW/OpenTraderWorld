-- MCP tokens can carry an expiry date. NULL = never expires (every token minted before
-- this migration), so nothing already configured stops working.
ALTER TABLE mcp_tokens ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;
