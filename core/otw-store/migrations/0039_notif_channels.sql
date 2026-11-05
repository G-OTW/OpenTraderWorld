-- RemindMe external notification channels.
--
-- Each reminder notification is written in-app (see notifications table). In addition,
-- the user can plug one or more third-party channels (email/telegram/slack/discord) that
-- a fired notification is *also* pushed to. Integration is free for the host: the user
-- brings their own account (SMTP creds, bot token, incoming webhook URL) and pays any
-- provider fees themselves.
--
-- A channel is only used when `enabled = TRUE`, so the user explicitly activates each
-- destination (all, none, or some). Non-secret settings live in `config` (JSONB);
-- the single secret (SMTP password / bot token / webhook URL) is sealed at rest with
-- XChaCha20-Poly1305 under OTW_SECRET_KEY — same scheme as feed secrets. Single-user:
-- no owner scoping.

CREATE TABLE IF NOT EXISTS notif_channels (
    id            UUID PRIMARY KEY,
    -- 'email' | 'telegram' | 'slack' | 'discord'.
    kind          TEXT NOT NULL
                  CHECK (kind IN ('email', 'telegram', 'slack', 'discord')),
    name          TEXT NOT NULL DEFAULT '',
    -- Non-secret config (e.g. email: host/port/from/to; telegram: chat_id). JSON object.
    config        JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Sealed secret: nonce + ciphertext. NULL until the user sets one.
    secret_nonce  BYTEA,
    secret_cipher BYTEA,
    -- User must explicitly turn a channel on before anything is sent to it.
    enabled       BOOLEAN NOT NULL DEFAULT FALSE,
    -- Last send outcome, for surfacing status in the UI. NULL until first attempt.
    last_ok       BOOLEAN,
    last_error    TEXT,
    last_sent_at  TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_notif_channels_enabled ON notif_channels(enabled) WHERE enabled;
