-- Per-conversation MCP token: each conversation carries its own tools envelope, switchable
-- from the chat header. The agent-level mcp_token_id remains as the DEFAULT copied into new
-- conversations at creation. NULL = no tools (chat only); a deleted token degrades the
-- conversation to chat via ON DELETE SET NULL.
ALTER TABLE agent_conversations
    ADD COLUMN mcp_token_id UUID REFERENCES mcp_tokens(id) ON DELETE SET NULL;

-- Existing conversations keep behaving as before: inherit the agent's current token.
UPDATE agent_conversations c
SET mcp_token_id = a.mcp_token_id
FROM agent_agents a
WHERE c.agent_id = a.id;
