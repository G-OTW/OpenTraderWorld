-- Personas: role-shaped agents (quant, portfolio manager, day trader, researcher, analyst).
--
-- The table was already multi-agent-ready; this adds the two columns a *shipped* preset needs:
--
--   slug     stable identity for the seeder. The user may rename a persona, so `name` cannot
--            carry identity: without a slug, re-seeding a renamed persona would duplicate it.
--            Empty for user-created agents (they need no shipped counterpart).
--   builtin  marks a row the app seeded, so the UI can offer "reset to shipped" and the
--            seeder knows what it owns. User-created agents are never touched by the seeder.
ALTER TABLE agent_agents
    ADD COLUMN slug    TEXT NOT NULL DEFAULT '',
    ADD COLUMN builtin BOOLEAN NOT NULL DEFAULT FALSE;

-- One row per shipped persona. Partial: user-created agents all share the empty slug.
CREATE UNIQUE INDEX agent_agents_slug ON agent_agents (slug) WHERE slug <> '';
