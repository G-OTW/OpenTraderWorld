-- Mailbox module: read-only IMAP ingest of newsletters, news mail and broker mail,
-- grouped by sender, plus a curated store of newsletter links.
--
-- Credentials never live here: `vault_item_id` points at the central vault, which is the
-- only place a password is (sealed) stored. The module never writes to the mail server:
-- it EXAMINEs the folder, reads new UIDs, and stores what the user chose to keep.

-- One connected mailbox. `security`: ssl = implicit TLS (993), starttls = upgrade on 143,
-- none = plaintext (loopback only, e.g. Proton Bridge).
CREATE TABLE IF NOT EXISTS mailbox_accounts (
    id             UUID PRIMARY KEY,
    name           TEXT NOT NULL DEFAULT '',
    email          TEXT NOT NULL DEFAULT '',
    preset         TEXT NOT NULL DEFAULT 'generic',
    host           TEXT NOT NULL,
    port           INTEGER NOT NULL DEFAULT 993,
    security       TEXT NOT NULL DEFAULT 'ssl' CHECK (security IN ('ssl', 'starttls', 'none')),
    username       TEXT NOT NULL,
    vault_item_id  UUID NOT NULL REFERENCES vault_items(id),
    folder         TEXT NOT NULL DEFAULT 'INBOX',
    interval_secs  INTEGER NOT NULL DEFAULT 900,
    enabled        BOOLEAN NOT NULL DEFAULT TRUE,
    -- IMAP sync bookkeeping: UIDs are only meaningful within one UIDVALIDITY.
    uid_validity   BIGINT NOT NULL DEFAULT 0,
    last_uid       BIGINT NOT NULL DEFAULT 0,
    last_poll_at   TIMESTAMPTZ,
    last_success_at TIMESTAMPTZ,
    last_error     TEXT,
    fail_count     INTEGER NOT NULL DEFAULT 0,
    next_run_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_mailbox_accounts_due ON mailbox_accounts(enabled, next_run_at);

-- One sender address seen in a connected mailbox.
--   category: how the user files it (news | newsletter | broker | other)
--   status:   kept    = its mail is stored
--             pending = seen but not list mail and not yet approved (nothing stored)
--             ignored = explicitly muted (nothing stored)
CREATE TABLE IF NOT EXISTS mailbox_senders (
    id             UUID PRIMARY KEY,
    from_addr      TEXT NOT NULL UNIQUE,
    domain         TEXT NOT NULL DEFAULT '',
    name           TEXT NOT NULL DEFAULT '',
    description    TEXT NOT NULL DEFAULT '',
    site_url       TEXT NOT NULL DEFAULT '',
    category       TEXT NOT NULL DEFAULT 'newsletter'
                   CHECK (category IN ('news', 'newsletter', 'broker', 'other')),
    status         TEXT NOT NULL DEFAULT 'kept'
                   CHECK (status IN ('kept', 'pending', 'ignored')),
    notify         BOOLEAN NOT NULL DEFAULT FALSE,
    list_unsubscribe TEXT NOT NULL DEFAULT '',
    last_subject   TEXT NOT NULL DEFAULT '',
    seen_count     INTEGER NOT NULL DEFAULT 0,
    first_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_mailbox_senders_domain ON mailbox_senders(domain);
CREATE INDEX IF NOT EXISTS idx_mailbox_senders_status ON mailbox_senders(status, category);

-- One stored message. `body_html` is sanitised at ingest (scripts/styles/forms stripped,
-- remote images parked in data-otw-src so nothing loads until the user asks).
CREATE TABLE IF NOT EXISTS mailbox_messages (
    id             UUID PRIMARY KEY,
    account_id     UUID NOT NULL REFERENCES mailbox_accounts(id) ON DELETE CASCADE,
    sender_id      UUID NOT NULL REFERENCES mailbox_senders(id) ON DELETE CASCADE,
    uid            BIGINT NOT NULL,
    uid_validity   BIGINT NOT NULL DEFAULT 0,
    message_id     TEXT NOT NULL DEFAULT '',
    subject        TEXT NOT NULL DEFAULT '',
    snippet        TEXT NOT NULL DEFAULT '',
    body_html      TEXT,
    body_text      TEXT,
    list_unsubscribe TEXT NOT NULL DEFAULT '',
    list_unsub_post BOOLEAN NOT NULL DEFAULT FALSE,
    has_remote_images BOOLEAN NOT NULL DEFAULT FALSE,
    attachment_count INTEGER NOT NULL DEFAULT 0,
    size_bytes     INTEGER NOT NULL DEFAULT 0,
    received_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    read           BOOLEAN NOT NULL DEFAULT FALSE,
    starred        BOOLEAN NOT NULL DEFAULT FALSE,
    archived       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, uid_validity, uid)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mailbox_messages_msgid
    ON mailbox_messages(account_id, message_id) WHERE message_id <> '';
CREATE INDEX IF NOT EXISTS idx_mailbox_messages_recent ON mailbox_messages(received_at DESC);
CREATE INDEX IF NOT EXISTS idx_mailbox_messages_sender ON mailbox_messages(sender_id, received_at DESC);
CREATE INDEX IF NOT EXISTS idx_mailbox_messages_unread ON mailbox_messages(read) WHERE NOT archived;

-- Real attachments (broker statements, PDFs…). Inline images referenced by the body are
-- not stored. Bytes live in the row: a mailbox holds statements, not media libraries.
CREATE TABLE IF NOT EXISTS mailbox_attachments (
    id          UUID PRIMARY KEY,
    message_id  UUID NOT NULL REFERENCES mailbox_messages(id) ON DELETE CASCADE,
    filename    TEXT NOT NULL DEFAULT 'attachment',
    mime        TEXT NOT NULL DEFAULT 'application/octet-stream',
    size_bytes  INTEGER NOT NULL DEFAULT 0,
    content     BYTEA NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_mailbox_attachments_msg ON mailbox_attachments(message_id);

-- The Store: the user's curated list of newsletters, one card per entry, opened in the
-- browser in one click. Independent from what is received (a link can exist with no
-- mailbox connected at all).
CREATE TABLE IF NOT EXISTS mailbox_store_links (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    url         TEXT NOT NULL DEFAULT '',
    domain      TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    topic       TEXT NOT NULL DEFAULT 'other'
                CHECK (topic IN ('mindset', 'finance', 'trading', 'geopolitics', 'economics', 'other')),
    subscribed  BOOLEAN NOT NULL DEFAULT FALSE,
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_mailbox_store_topic ON mailbox_store_links(topic, position);

-- Singleton module settings.
CREATE TABLE IF NOT EXISTS mailbox_settings (
    id                    INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    default_interval_secs INTEGER NOT NULL DEFAULT 900,
    max_body_kb           INTEGER NOT NULL DEFAULT 512,
    max_attachment_kb     INTEGER NOT NULL DEFAULT 10240,
    keep_attachments      BOOLEAN NOT NULL DEFAULT TRUE,
    load_remote_images    BOOLEAN NOT NULL DEFAULT FALSE,
    notify_on_new         BOOLEAN NOT NULL DEFAULT FALSE,
    -- 0 = keep everything.
    retention_days        INTEGER NOT NULL DEFAULT 0,
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO mailbox_settings (id) VALUES (1) ON CONFLICT (id) DO NOTHING;
