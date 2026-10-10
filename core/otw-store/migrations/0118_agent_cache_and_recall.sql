-- Agent: prompt-cache accounting + memory recall.
--
-- Cache columns: a cached prompt is billed differently from a fresh one (a write costs more
-- than a plain input token, a read costs a fraction), so the two have to be counted apart or
-- the usage badge reports a cost nobody is paying. Both providers report them; we stored
-- neither.
ALTER TABLE agent_messages
    ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN cache_read_tokens  INTEGER NOT NULL DEFAULT 0;

-- Memory recall: when the store is full, the prune suggestion has to name the entries nobody
-- has read in months rather than the oldest ones. Never used = NULL, which sorts first.
ALTER TABLE agent_memories
    ADD COLUMN last_used_at TIMESTAMPTZ;

-- memory_search scans slug + description + content of at most MEMORY_MAX_COUNT rows, so a
-- plain index on the recall order is all it needs.
CREATE INDEX agent_memories_recall ON agent_memories (last_used_at NULLS FIRST, updated_at);
