-- Agent: attribute each billed turn to what actually produced it.
--
-- Model, provider and persona live on the conversation and are all switchable at any time
-- (a mid-conversation model change, a persona reassignment). Reading usage back through the
-- conversation therefore re-attributes the whole history to the latest choice: a month spent
-- on one model then a switch, and the breakdown credits every token to the new one. Stamped
-- on the row, the answer stays true whatever is changed afterwards.
--
-- The ids stay foreign keys so a rename follows by itself; a deletion nulls them and the row
-- falls into an "unknown" bucket rather than vanishing from the totals.
ALTER TABLE agent_messages
    ADD COLUMN model       TEXT NOT NULL DEFAULT '',
    ADD COLUMN provider_id UUID REFERENCES agent_providers(id) ON DELETE SET NULL,
    ADD COLUMN agent_id    UUID REFERENCES agent_agents(id) ON DELETE SET NULL;

-- Rows written before this migration carry no stamp; the breakdown reports them apart rather
-- than guessing. Only assistant rows are billed, so that is all the usage query scans.
CREATE INDEX agent_messages_usage ON agent_messages (created_at) WHERE role = 'assistant';
