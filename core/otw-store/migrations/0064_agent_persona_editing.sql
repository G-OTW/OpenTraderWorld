-- Persona editing: skill allowlists by id, memory provenance, a delete that keeps history,
-- and the per-conversation simulation budget.

-- 1. The skill allowlist moves from names to ids, and "everything" becomes explicit.
--
-- `agent_agents.skills` held skill NAMES, matched against `agent_skills.name`. Once the user
-- can rename a skill in the skills pane, that match breaks silently: the skill vanishes from
-- every persona that declared it, with no error anywhere. Ids survive a rename.
--
-- The empty array meant "every enabled skill". That was safe while no one could edit a shelf;
-- it stops being safe the moment the editor exists, because removing a persona's last skill
-- would hand it the whole catalog. "Everything" is now the explicit marker `["*"]`, and an
-- empty array means an empty shelf.
UPDATE agent_agents a
SET skills = COALESCE(
        (SELECT jsonb_agg(s.id::text ORDER BY s.name)
           FROM jsonb_array_elements_text(a.skills) AS n(nm)
           JOIN agent_skills s ON s.name = n.nm),
        '[]'::jsonb)
WHERE jsonb_typeof(a.skills) = 'array' AND jsonb_array_length(a.skills) > 0;

-- Anything that had no shelf — the default Assistant, and any persona whose names all failed
-- to resolve — keeps today's behaviour: the whole catalog.
UPDATE agent_agents
SET skills = '["*"]'::jsonb
WHERE jsonb_typeof(skills) <> 'array' OR jsonb_array_length(skills) = 0;

-- 2. Deleting a persona must not delete what it said.
--
-- The FK cascaded, so removing a persona would take every conversation it ever held with it.
-- RESTRICT instead: the API reassigns those conversations to the default agent first (and
-- drops a marker in each transcript), so the history stays readable and attributable.
ALTER TABLE agent_conversations DROP CONSTRAINT agent_conversations_agent_id_fkey;
ALTER TABLE agent_conversations
    ADD CONSTRAINT agent_conversations_agent_id_fkey
    FOREIGN KEY (agent_id) REFERENCES agent_agents(id) ON DELETE RESTRICT;

-- 3. Memory provenance: which persona wrote this fact.
--
-- Memory is one shared store — the PM reads what the quant recorded — so a persona-specific
-- observation must not read as a global truth. NULL = written by the user, or by an agent
-- since deleted. Shown in the manager AND in the index injected into every system prompt,
-- because provenance the model cannot see is provenance that changes nothing.
ALTER TABLE agent_memories
    ADD COLUMN agent_id UUID REFERENCES agent_agents(id) ON DELETE SET NULL;

-- 4. Simulation budget, per conversation (R11).
--
-- A grid search is N runs, and nothing bounded the N. Counts every simulation this
-- conversation has asked for — /backtest/run calls and each trial of a /backtest/sweep.
ALTER TABLE agent_conversations
    ADD COLUMN backtest_runs INT NOT NULL DEFAULT 0;
