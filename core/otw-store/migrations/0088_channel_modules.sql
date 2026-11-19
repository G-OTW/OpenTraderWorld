-- Notification channels: per-module grants.
--
-- Channels (0039) were born inside RemindMe and stayed global: every producer that
-- fired a notification pushed it to *every* enabled channel. Since then watchlists,
-- mailbox and inbound webhooks all push too, so "enabled" became the only dial —
-- the user could not say "price alerts to Telegram, everything else to email".
--
-- Grants now live here, one row per allowed module id, exactly like `connector_modules`
-- does for the data broker. A row with module '*' grants every notifying module,
-- present and future.

CREATE TABLE IF NOT EXISTS channel_modules (
    channel_id UUID NOT NULL REFERENCES notif_channels(id) ON DELETE CASCADE,
    module     TEXT NOT NULL,
    PRIMARY KEY (channel_id, module)
);

-- Existing channels keep exactly the reach they had: everything.
INSERT INTO channel_modules (channel_id, module)
SELECT id, '*' FROM notif_channels
ON CONFLICT DO NOTHING;
