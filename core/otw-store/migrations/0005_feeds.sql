-- News feed module: sources (feeds), their fetched items, and encrypted secrets.

-- A feed is one configured source. `kind` selects the connector (rss | api).
-- `config` holds connector-specific settings as JSON:
--   rss: { "url": "https://…" }
--   api: { "url", "method", "headers": {..}, "query": {..},
--          "items_path", "title_path", "url_path", "date_path",
--          "summary_path", "source_path" }
-- Secret-bearing values in config reference a secret by name (see feed_secrets),
-- e.g. a header value of "{{secret:api_key}}" is substituted at fetch time.
CREATE TABLE IF NOT EXISTS feeds (
    id             UUID PRIMARY KEY,
    name           TEXT NOT NULL DEFAULT '',
    kind           TEXT NOT NULL CHECK (kind IN ('rss', 'api')),
    config         JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled        BOOLEAN NOT NULL DEFAULT TRUE,
    -- Poll cadence in seconds (NULL = use the module default).
    interval_secs  INTEGER NOT NULL DEFAULT 900,
    -- Health / scheduling bookkeeping.
    last_fetched_at TIMESTAMPTZ,
    last_success_at TIMESTAMPTZ,
    last_error      TEXT,
    next_run_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_feeds_due ON feeds(enabled, next_run_at);

-- Items fetched from feeds. `dedup_key` is a stable hash (guid/url) used to
-- avoid re-inserting the same item on every poll.
CREATE TABLE IF NOT EXISTS feed_items (
    id           UUID PRIMARY KEY,
    feed_id      UUID NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    dedup_key    TEXT NOT NULL,
    title        TEXT NOT NULL DEFAULT '',
    url          TEXT,
    summary      TEXT,
    source_name  TEXT NOT NULL DEFAULT '',
    source_type  TEXT NOT NULL DEFAULT '',
    published_at TIMESTAMPTZ,
    raw          JSONB,
    fetched_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (feed_id, dedup_key)
);
CREATE INDEX IF NOT EXISTS idx_feed_items_published ON feed_items(published_at DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_feed_items_feed ON feed_items(feed_id);
CREATE INDEX IF NOT EXISTS idx_feed_items_source_type ON feed_items(source_type);

-- Encrypted secrets attached to a feed (API keys, tokens…). The plaintext is
-- never stored or returned: `ciphertext` is XChaCha20-Poly1305 AEAD output and
-- `nonce` is unique per secret. The API exposes only secret names + "is set".
CREATE TABLE IF NOT EXISTS feed_secrets (
    id         UUID PRIMARY KEY,
    feed_id    UUID NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    nonce      BYTEA NOT NULL,
    ciphertext BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (feed_id, name)
);
