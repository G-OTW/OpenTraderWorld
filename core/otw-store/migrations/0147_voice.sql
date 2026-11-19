-- Voice control: speech engines (the voice broker) and spoken commands.
--
-- An engine turns recorded audio into text. The browser's own recognizer needs no row (it
-- runs client side); every server engine is one row here, its key sealed like the agent
-- providers' keys or plugged from the vault.
CREATE TABLE voice_engines (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind               TEXT NOT NULL CHECK (kind IN ('openai_compat', 'whisper_cpp')),
    label              TEXT NOT NULL,
    base_url           TEXT NOT NULL DEFAULT '',
    api_key            TEXT NOT NULL DEFAULT '',
    api_key_vault_item UUID REFERENCES vault_items(id),
    model              TEXT NOT NULL DEFAULT '',
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A spoken command: a phrase (plus alternative phrasings) mapped to an ordered list of
-- steps. A phrase fires only on a whole-segment match, and a plan always stops on the
-- confirmation dialog unless the command opts out with bypass_confirm.
CREATE TABLE voice_commands (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    phrase         TEXT NOT NULL,
    aliases        TEXT[] NOT NULL DEFAULT '{}',
    steps          JSONB NOT NULL DEFAULT '[]',
    bypass_confirm BOOLEAN NOT NULL DEFAULT FALSE,
    enabled        BOOLEAN NOT NULL DEFAULT TRUE,
    position       INTEGER NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
