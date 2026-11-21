-- Write consent, per persona.
--
-- 0052 dropped a column of this name, and for a good reason: it was a second PERMISSION gate
-- stacked on the MCP token, which already decides what the agent may touch. This one is not
-- that.
--
-- Permission is unchanged — the token still decides what is reachable. This column says
-- whether the user wants to be ASKED each time before an allowed write happens: a preference
-- about being interrupted, not about access. Default FALSE, so writes confirm out of the box.
--
-- Deletes confirm regardless of the flag (see `agent::confirm`), so ticking it can never
-- widen what a single mistake costs.
ALTER TABLE agent_agents
    ADD COLUMN auto_approve_writes BOOLEAN NOT NULL DEFAULT FALSE;
