-- Tool rounds one message may spend before the run wraps up, per agent. NULL = the built-in
-- default (agent::run::MAX_TOOL_ITERS).
ALTER TABLE agent_agents ADD COLUMN max_tool_rounds INTEGER;
