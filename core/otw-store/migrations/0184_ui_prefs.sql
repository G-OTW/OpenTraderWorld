-- User interface preferences: theme, privacy mask, rail widths, picks and the like.
--
-- One row per preference key, the value kept as the JSON the frontend reads back. These
-- used to live only in the browser's localStorage and were lost on another browser; the
-- frontend now keeps localStorage as a first-paint mirror of this table. Distinct from
-- app_settings, which holds seeded application configuration as TEXT.

CREATE TABLE IF NOT EXISTS ui_prefs (
    key        TEXT PRIMARY KEY,
    value      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
