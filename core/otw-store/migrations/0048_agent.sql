-- Agent module — a light chat agent inside OTW (ChatGPT-like), provider-agnostic.
--
-- Six tables under the agent_* prefix. v0 ships a single default agent (row auto-seeded
-- lazily by the API on first use); the schema is multi-agent-ready for v1 (personas,
-- per-task agents). Provider API keys are write-only: stored here, never returned by GET.

-- Providers: one of two wire formats — "anthropic" (Messages API) or "openai_compat"
-- (/chat/completions, serves OpenRouter/OpenAI/DeepSeek/Moonshot/Groq/Mistral/Gemini-compat…).
-- No vendor is privileged: the active provider+model come entirely from user config.
CREATE TABLE agent_providers (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind         TEXT NOT NULL CHECK (kind IN ('anthropic', 'openai_compat')),
    label        TEXT NOT NULL,
    base_url     TEXT NOT NULL DEFAULT '',
    api_key      TEXT NOT NULL DEFAULT '',
    default_model TEXT NOT NULL DEFAULT '',
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Agents: v0 exposes one default agent (is_default). params JSONB holds provider-specific
-- knobs (max_tokens, reasoning effort…) so no vendor field is ever a column.
CREATE TABLE agent_agents (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT NOT NULL,
    system_prompt TEXT NOT NULL DEFAULT '',
    provider_id   UUID REFERENCES agent_providers(id) ON DELETE SET NULL,
    model         TEXT NOT NULL DEFAULT '',
    params        JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- FK to the MCP token that is this agent's permission envelope (Phase 2).
    mcp_token_id  UUID REFERENCES mcp_tokens(id) ON DELETE SET NULL,
    -- Skills allowlist (names); Phase 3.
    skills        JSONB NOT NULL DEFAULT '[]'::jsonb,
    -- Auto-approve otw_write tool calls instead of showing a confirm chip (Phase 2).
    auto_approve_writes BOOLEAN NOT NULL DEFAULT FALSE,
    is_default    BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- At most one default agent.
CREATE UNIQUE INDEX agent_agents_one_default ON agent_agents (is_default) WHERE is_default;

CREATE TABLE agent_conversations (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id   UUID NOT NULL REFERENCES agent_agents(id) ON DELETE CASCADE,
    title      TEXT NOT NULL DEFAULT '',
    -- Rolling summary of older turns (Phase 3); empty until the threshold trips.
    summary    TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX agent_conversations_agent ON agent_conversations (agent_id, updated_at DESC);

-- Messages: content is a normalized block array (text | tool_use | tool_result | thinking),
-- stored as JSONB so adapters translate both directions without lossy column mapping.
CREATE TABLE agent_messages (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES agent_conversations(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'tool', 'system')),
    content         JSONB NOT NULL DEFAULT '[]'::jsonb,
    input_tokens    INTEGER NOT NULL DEFAULT 0,
    output_tokens   INTEGER NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX agent_messages_conv ON agent_messages (conversation_id, created_at);

-- Long-term light memory (Phase 3): small md facts the agent writes via memory_write.
CREATE TABLE agent_memories (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    content     TEXT NOT NULL DEFAULT '',
    kind        TEXT NOT NULL DEFAULT 'note',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Skills (Phase 3): SKILL.md-like md instructions; catalog (name+description) is injected,
-- body loaded on demand via load_skill.
CREATE TABLE agent_skills (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    body        TEXT NOT NULL DEFAULT '',
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    builtin     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
