-- External control: driving OTW from a chat channel (Telegram, Slack, Discord).
--
-- The outbound half already exists (`notif_channels`): a channel is a destination the user
-- owns. This is the return path, and it is deliberately NOT a second channel list. A
-- control binding points at an existing channel and adds the three things an inbound leg
-- needs and an outbound one does not: a bot credential that can receive (a webhook URL
-- cannot), the agent that answers, and the permission envelope the answers run under.
--
-- No new authorization layer. The envelope is an `mcp_tokens` row, exactly as the in-app
-- agent uses; `external` is a flag on that row saying the token may be reached from
-- outside, opt-in per token and never acquired by migration. One token per binding and one
-- binding per channel: a token shared between Telegram and Slack would make revoking one
-- revoke the other, and the audit trail would stop saying which way a call came in.

ALTER TABLE mcp_tokens
    ADD COLUMN IF NOT EXISTS external BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE IF NOT EXISTS control_bindings (
    id                UUID PRIMARY KEY,
    name              TEXT NOT NULL,
    -- The channel this binding listens on and replies through.
    channel_id        UUID NOT NULL UNIQUE REFERENCES notif_channels(id) ON DELETE CASCADE,
    agent_id          UUID NOT NULL REFERENCES agent_agents(id) ON DELETE CASCADE,
    -- The permission ceiling. Restricted to tokens carrying `external`.
    token_id          UUID NOT NULL UNIQUE REFERENCES mcp_tokens(id) ON DELETE CASCADE,
    enabled           BOOLEAN NOT NULL DEFAULT false,
    -- Bot credential for the INBOUND leg: a BotFather token, a Slack app-level token, a
    -- Discord bot token. Distinct from the channel's outbound secret, which is usually a
    -- webhook URL that cannot receive anything. Local or plugged from the vault.
    secret_nonce      BYTEA,
    secret_cipher     BYTEA,
    secret_vault_item UUID REFERENCES vault_items(id),
    -- Non-secret transport knobs.
    config            JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Pairing code, in clear because the user has to read it off the screen and type it
    -- into the chat. Single use, minutes-long, and useless once redeemed.
    pair_code         TEXT,
    pair_expires_at   TIMESTAMPTZ,
    -- Transport health, same shape as a channel's send status.
    last_ok           BOOLEAN,
    last_error        TEXT,
    last_seen_at      TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Who may drive. A channel is a destination, not an identity: anyone in a Telegram group
-- can post to the chat the bot is in, so authority is bound to the platform's sender id
-- and nothing else. An empty list means the binding answers nobody.
CREATE TABLE IF NOT EXISTS control_senders (
    binding_id UUID NOT NULL REFERENCES control_bindings(id) ON DELETE CASCADE,
    sender_id  TEXT NOT NULL,
    label      TEXT NOT NULL DEFAULT '',
    paired_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (binding_id, sender_id)
);

-- One agent conversation per chat, so a thread keeps its context between messages instead
-- of restarting cold on every line the user sends.
CREATE TABLE IF NOT EXISTS control_chats (
    binding_id      UUID NOT NULL REFERENCES control_bindings(id) ON DELETE CASCADE,
    chat_id         TEXT NOT NULL,
    conversation_id UUID NOT NULL REFERENCES agent_conversations(id) ON DELETE CASCADE,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (binding_id, chat_id)
);
CREATE INDEX IF NOT EXISTS idx_control_chats_conversation ON control_chats(conversation_id);

-- Where each transport got to in its own stream (a Telegram update offset, a Slack/Discord
-- resume point), so a restart does not replay messages that were already answered.
CREATE TABLE IF NOT EXISTS control_cursors (
    binding_id UUID PRIMARY KEY REFERENCES control_bindings(id) ON DELETE CASCADE,
    cursor     TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
