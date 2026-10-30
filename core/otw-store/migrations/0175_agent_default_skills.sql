-- The default assistant was seeded with an empty shelf, so it never saw a skill. It gets the
-- whole catalog, unless the user already put something on its shelf.
UPDATE agent_agents SET skills = '["*"]'::jsonb WHERE is_default AND skills = '[]'::jsonb;

-- SHA-256 hex of the shipped content last written by the seeder. A row whose current content
-- still hashes to it was never edited, so the next boot may bring it up to the shipped version.
-- NULL on rows seeded before this column: the seeder treats those as never edited only when
-- created_at = updated_at, or, for a prompt, when it still equals a text an earlier build
-- shipped.
ALTER TABLE agent_skills ADD COLUMN shipped_hash TEXT;
ALTER TABLE agent_agents ADD COLUMN shipped_skills_hash TEXT;
ALTER TABLE agent_agents ADD COLUMN shipped_prompt_hash TEXT;
