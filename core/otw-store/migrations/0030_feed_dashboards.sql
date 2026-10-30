-- News feed v0.2: feed dashboards.
--
-- A "feed dashboard" is a named collection of sources. The existing `feeds`
-- table keeps holding the sources themselves (one row = one configured source);
-- a many-to-many join (`dashboard_sources`) places a source into one or more
-- dashboards, each carrying its own poll interval for that source.
--
-- Polling is now driven by dashboards, not by the source's own `enabled` flag:
-- a source is polled iff at least one *started* dashboard references it, at the
-- shortest interval among those started dashboards. The source's `interval_secs`
-- column is retained only as a default for new links.

CREATE TABLE IF NOT EXISTS feed_dashboards (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL DEFAULT '',
    -- Exactly one dashboard may be the default (see partial unique index below).
    is_default  BOOLEAN NOT NULL DEFAULT FALSE,
    -- Play/stop: when started, its sources are auto-polled by the scheduler.
    started     BOOLEAN NOT NULL DEFAULT TRUE,
    -- Favorited dashboards appear in the left-pane shortcut strip.
    favorite    BOOLEAN NOT NULL DEFAULT FALSE,
    -- Manual ordering within the favorites strip / dropdown.
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- At most one default dashboard.
CREATE UNIQUE INDEX IF NOT EXISTS uq_feed_dashboards_default
    ON feed_dashboards (is_default) WHERE is_default;

-- Join: which sources belong to which dashboard, with a per-link poll interval.
CREATE TABLE IF NOT EXISTS dashboard_sources (
    dashboard_id  UUID NOT NULL REFERENCES feed_dashboards(id) ON DELETE CASCADE,
    feed_id       UUID NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    interval_secs INTEGER NOT NULL DEFAULT 900,
    position      INTEGER NOT NULL DEFAULT 0,
    added_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (dashboard_id, feed_id)
);
CREATE INDEX IF NOT EXISTS idx_dashboard_sources_feed ON dashboard_sources(feed_id);

-- Dedup hash over the source's identity (kind + normalized config + secret
-- names), computed in Rust. Lets the UI detect "this source already exists in
-- dashboard X — reuse the same settings?" before creating a duplicate. Nullable
-- so pre-existing rows backfill lazily on next save.
ALTER TABLE feeds ADD COLUMN IF NOT EXISTS dedup_hash TEXT;
CREATE INDEX IF NOT EXISTS idx_feeds_dedup ON feeds(dedup_hash);

-- Migration (Option A): fold all existing sources into one started "Default"
-- dashboard so nothing is lost. Only runs when there are no dashboards yet.
DO $$
DECLARE
    d UUID;
BEGIN
    IF NOT EXISTS (SELECT 1 FROM feed_dashboards) THEN
        d := gen_random_uuid();
        INSERT INTO feed_dashboards (id, name, is_default, started, favorite)
            VALUES (d, 'Default', TRUE, TRUE, TRUE);
        INSERT INTO dashboard_sources (dashboard_id, feed_id, interval_secs)
            SELECT d, id, interval_secs FROM feeds;
    END IF;
END $$;
