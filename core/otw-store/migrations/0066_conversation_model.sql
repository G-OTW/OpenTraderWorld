-- Provider + model per conversation.
--
-- Until now the model picker in the chat header wrote to the default agent, so switching
-- model in one thread switched it in every thread. That is wrong for the thing people
-- actually do: run a cheap fast model for a journal review in one tab and the strongest
-- reasoning model for a backtest argument in another, at the same time.
--
-- NULL / empty means "inherit" — the persona's choice, then the default agent's, then the
-- provider's own default model. Nothing is snapshotted: a conversation that never overrode
-- anything keeps following the agent settings, which is what it did before this column.
ALTER TABLE agent_conversations
    ADD COLUMN provider_id UUID REFERENCES agent_providers(id) ON DELETE SET NULL,
    ADD COLUMN model       TEXT NOT NULL DEFAULT '';
