-- External control: the binding picks its own provider and model.
--
-- A binding already names the agent that answers, and the agent carries a provider/model.
-- That is the right default for the app, but a chat is a different place: the phone leg
-- wants a cheap fast model while the same persona keeps a large one in the browser, and
-- the person driving it is not in front of the settings screen. So the binding gets its
-- own override, seeded into every conversation it opens and switchable from the chat
-- itself (`/provider`, `/model`).
--
-- NULL provider + empty model = inherit, exactly as `agent_conversations` already does.

ALTER TABLE control_bindings
    ADD COLUMN IF NOT EXISTS provider_id UUID REFERENCES agent_providers(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS model TEXT NOT NULL DEFAULT '';
