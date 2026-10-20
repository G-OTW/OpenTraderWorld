-- Fills a paper session took on the live price between two runs: a stop, a target or a limit
-- reached while the bar was still forming. The run that follows replays each one on its bar,
-- as given, instead of re-deciding it from the candle, and does not alert it a second time.
CREATE TABLE IF NOT EXISTS paper_live_fills (
    id         UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES paper_sessions(id) ON DELETE CASCADE,
    trade_key  TEXT NOT NULL,
    ticker     TEXT NOT NULL,
    -- entry | exit
    kind       TEXT NOT NULL,
    direction  TEXT NOT NULL,
    reason     TEXT NOT NULL,
    -- The strategy bar it happened in, stamped as the engine stamps its bars.
    bar_ts     TEXT NOT NULL,
    price      DOUBLE PRECISION NOT NULL,
    maker      BOOLEAN NOT NULL,
    quoted     BOOLEAN NOT NULL,
    at         TIMESTAMPTZ NOT NULL,
    UNIQUE (session_id, trade_key, kind)
);
