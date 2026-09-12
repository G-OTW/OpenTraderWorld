-- Price alerts on watchlist items.
--
-- An alert watches one item and fires when its quote crosses a condition. Evaluation runs
-- server-side inside the existing watchlist refresh loop, so an alert fires whether or not
-- a browser is open — that is the whole point of the feature.
--
-- Three metrics, deliberately no more:
--   price  — the quote crosses an absolute threshold (above / below).
--   pct    — the quote moved by ±N% away from a reference.
--   usd    — the quote moved by ±N USD away from a reference.
--
-- What the reference *is* is the `basis` column, and it is the one thing we refuse to guess:
--   anchor  — the price at the moment the alert was armed, frozen in `ref_price`. Reads as
--             "from now on, tell me when it moves 5%". `window_secs > 0` gives that question
--             a deadline (`expires_at`); 0 means it stands until it fires.
--   rolling — the price as it was `window_secs` ago, re-read on every evaluation. Reads as
--             "tell me whenever it moves 5% within 24h". Needs price history, which is why
--             watchlist_samples exists below.
--
-- `channel_ids` is the per-alert destination choice. Empty = every enabled channel (the
-- reminder behaviour). Non-empty = only those, even if other channels are enabled and
-- healthy — the user picks per alert, the channels themselves stay shared and configured
-- once in RemindMe.

CREATE TABLE IF NOT EXISTS watchlist_alerts (
    id            UUID PRIMARY KEY,
    item_id       UUID NOT NULL REFERENCES watchlist_items(id) ON DELETE CASCADE,
    metric        TEXT NOT NULL CHECK (metric IN ('price', 'pct', 'usd')),
    -- price: 'above' | 'below'. pct/usd: 'up' | 'down' | 'move' (either direction).
    direction     TEXT NOT NULL CHECK (direction IN ('above', 'below', 'up', 'down', 'move')),
    -- Absolute price for 'price'; magnitude of the move (always positive) for pct/usd.
    threshold     DOUBLE PRECISION NOT NULL,
    -- 'anchor' | 'rolling'. Ignored for the 'price' metric.
    basis         TEXT NOT NULL DEFAULT 'anchor' CHECK (basis IN ('anchor', 'rolling')),
    -- Window in seconds. anchor: lifetime (0 = no deadline). rolling: how far back to look.
    window_secs   INTEGER NOT NULL DEFAULT 0 CHECK (window_secs >= 0),
    -- Frozen reference for basis='anchor', re-armed after each fire when repeating.
    ref_price     DOUBLE PRECISION,
    armed_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Deadline for basis='anchor' with a window; NULL = stands until it fires.
    expires_at    TIMESTAMPTZ,
    -- FALSE: fires once, then disables itself. TRUE: re-arms, honouring cooldown_secs.
    repeat        BOOLEAN NOT NULL DEFAULT FALSE,
    cooldown_secs INTEGER NOT NULL DEFAULT 3600 CHECK (cooldown_secs >= 0),
    enabled       BOOLEAN NOT NULL DEFAULT TRUE,
    -- Destination channels. Empty array = all enabled channels.
    channel_ids   UUID[] NOT NULL DEFAULT '{}',
    note          TEXT NOT NULL DEFAULT '',
    last_fired_at TIMESTAMPTZ,
    last_price    DOUBLE PRECISION,
    fire_count    INTEGER NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_watchlist_alerts_item ON watchlist_alerts(item_id);
CREATE INDEX IF NOT EXISTS idx_watchlist_alerts_live ON watchlist_alerts(item_id) WHERE enabled;

-- Price history, sampled at refresh cadence — the only thing a rolling window can be
-- computed against (the cached quote carries daily closes, which cannot answer "in the
-- last 6 hours"). Written *only* for items carrying an enabled rolling alert, so a user
-- who never arms one pays nothing. Purged to the longest window that still needs it.
CREATE TABLE IF NOT EXISTS watchlist_samples (
    item_id UUID NOT NULL REFERENCES watchlist_items(id) ON DELETE CASCADE,
    ts      TIMESTAMPTZ NOT NULL DEFAULT now(),
    price   DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (item_id, ts)
);
CREATE INDEX IF NOT EXISTS idx_watchlist_samples_lookup ON watchlist_samples(item_id, ts DESC);

-- Notifications already carry kind/linked_id/url; a fired alert reuses them with
-- kind='watchlist' and linked_id = the item. No schema change needed there.
