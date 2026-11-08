-- Agent: per-chat spend limits.
--
-- A conversation can be capped on output tokens, on dollars, or both. The default agent row
-- holds the limits every NEW conversation starts with; each conversation keeps its own copy,
-- adjustable from the chat, so changing the default never rewrites a running chat. NULL means
-- unlimited.
ALTER TABLE agent_agents
    ADD COLUMN limit_output_tokens INTEGER,
    ADD COLUMN limit_cost_usd      DOUBLE PRECISION;

ALTER TABLE agent_conversations
    ADD COLUMN limit_output_tokens INTEGER,
    ADD COLUMN limit_cost_usd      DOUBLE PRECISION;

-- What a turn cost, in USD, as the provider reported it (OpenRouter returns it with the
-- usage block). NULL when the provider sent no price: never estimated from a rate table.
ALTER TABLE agent_messages
    ADD COLUMN cost_usd DOUBLE PRECISION;
