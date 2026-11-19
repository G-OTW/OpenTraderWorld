-- Managers' Portfolios — user snapshots.
--
-- A snapshot is an immutable, point-in-time copy of one manager's portfolio and its holdings,
-- taken by the user via the camera icon. Unlike manager_portfolios (which the scheduled Dataroma
-- scrape overwrites wholesale), snapshots are frozen: they carry no foreign key to the live
-- portfolio so a later refresh or portfolio delete never mutates or cascades into them. The same
-- portfolio can be snapshotted any number of times; each is a distinct dated row. The user may
-- delete an individual snapshot or every snapshot for a given portfolio (by source_slug).

CREATE TABLE IF NOT EXISTS manager_snapshots (
    id            UUID PRIMARY KEY,
    -- The upstream manager code the snapshot was taken from (manager_portfolios.slug at the time).
    -- Groups snapshots of the same portfolio together; not a foreign key (snapshots outlive rows).
    source_slug   TEXT NOT NULL,
    -- Frozen copy of the portfolio label + summary fields as they were when snapped.
    name          TEXT NOT NULL,
    value_text    TEXT NOT NULL DEFAULT '',
    value_num     DOUBLE PRECISION,
    stock_count   INTEGER NOT NULL DEFAULT 0,
    period        TEXT NOT NULL DEFAULT '',
    source_url    TEXT NOT NULL DEFAULT '',
    -- When the user took the snapshot.
    taken_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS manager_snapshot_holdings (
    id              UUID PRIMARY KEY,
    snapshot_id     UUID NOT NULL REFERENCES manager_snapshots(id) ON DELETE CASCADE,
    position        INTEGER NOT NULL DEFAULT 0,
    ticker          TEXT NOT NULL DEFAULT '',
    company         TEXT NOT NULL DEFAULT '',
    pct             DOUBLE PRECISION,
    activity        TEXT NOT NULL DEFAULT '',
    shares          DOUBLE PRECISION,
    reported_price  DOUBLE PRECISION,
    value           DOUBLE PRECISION,
    current_price   DOUBLE PRECISION,
    change_pct      DOUBLE PRECISION,
    week52_low      DOUBLE PRECISION,
    week52_high     DOUBLE PRECISION
);

CREATE INDEX IF NOT EXISTS idx_manager_snapshot_holdings_snapshot
    ON manager_snapshot_holdings(snapshot_id);
-- The Snapshots tab groups by source_slug and orders newest-first.
CREATE INDEX IF NOT EXISTS idx_manager_snapshots_slug ON manager_snapshots(source_slug, taken_at DESC);
