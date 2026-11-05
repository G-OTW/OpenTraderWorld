-- API rate tracking.
--
-- Tracks outbound calls to external providers (market data, FX, feeds, quotes) so the
-- Settings "API Rate" dashboard can show request volume per provider "since the beginning
-- of the day" and flag over-limit (HTTP 429 / "too many requests") responses. Nothing here
-- throttles or blocks anything — it is observe-and-alert only.
--
-- api_rate_daily: one row per (provider, host, day). Counters are upserted-incremented per
-- call, so volume stays a single cheap UPDATE regardless of request rate. `limited` counts
-- how many of those calls came back rate-limited. Retention: 30 days of rollups.
--
-- api_rate_events: one row per over-limit hit, for the recent-hits list. Bounded by trim.

CREATE TABLE IF NOT EXISTS api_rate_daily (
    provider  TEXT NOT NULL,
    host      TEXT NOT NULL DEFAULT '',
    day       DATE NOT NULL DEFAULT (now() AT TIME ZONE 'UTC')::date,
    requests  BIGINT NOT NULL DEFAULT 0,   -- total outbound calls recorded
    limited   BIGINT NOT NULL DEFAULT 0,   -- of those, how many were rate-limited
    errors    BIGINT NOT NULL DEFAULT 0,   -- of those, how many were other failures (>=400, network)
    last_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, host, day)
);
CREATE INDEX IF NOT EXISTS idx_api_rate_daily_day ON api_rate_daily(day DESC);

CREATE TABLE IF NOT EXISTS api_rate_events (
    id        BIGSERIAL PRIMARY KEY,
    at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    provider  TEXT NOT NULL,
    host      TEXT NOT NULL DEFAULT '',
    status    INTEGER,                     -- HTTP status if any (429), NULL for body-level notes
    detail    TEXT NOT NULL DEFAULT ''     -- provider message / context
);
CREATE INDEX IF NOT EXISTS idx_api_rate_events_at ON api_rate_events(at DESC);
