-- Settings + application logs.
--
-- app_settings: a flat key/value store for global, user-editable settings (default
-- currency, default timezone, minimum log level, …). Single-user, so one row per key.
-- Values are stored as TEXT; callers interpret them. Modules will read these later.
--
-- app_logs: structured application logs persisted from the tracing layer so the Settings
-- "Logs" section can fetch and filter them in-app, independent of the deploy/host. Volume
-- is bounded by a retention trim on insert.

CREATE TABLE IF NOT EXISTS app_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed defaults (only if absent).
INSERT INTO app_settings (key, value) VALUES
    ('default_currency', 'USD'),
    ('default_timezone', 'UTC'),
    ('log_level', 'info')
ON CONFLICT (key) DO NOTHING;

CREATE TABLE IF NOT EXISTS app_logs (
    id      BIGSERIAL PRIMARY KEY,
    at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    level   TEXT NOT NULL,          -- error | warn | info | debug | trace
    target  TEXT NOT NULL DEFAULT '',
    message TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_app_logs_at ON app_logs(at DESC);
CREATE INDEX IF NOT EXISTS idx_app_logs_level ON app_logs(level);
