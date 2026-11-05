-- Drop the agent-level "allow writes" toggle. It was a second gate stacked on top of the
-- MCP token's per-module permissions — but the token (Settings → AI agents) IS the
-- permission envelope: r / rw / rwd per module apply directly to the agent's OTW tools,
-- with no agent-side re-validation. External MCP servers keep their own warnings.
ALTER TABLE agent_agents DROP COLUMN auto_approve_writes;
