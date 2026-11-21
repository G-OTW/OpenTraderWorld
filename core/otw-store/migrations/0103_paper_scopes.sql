-- Paper trading: what a message may say, and which fills are worth one.
--
-- Two things this adds, and one it fixes.
--
-- 1. A **period** to talk about. `seeded_at` is the instant the session learned its own book,
--    so "since the beginning" means since the session existed and not since the oldest candle
--    in the window. `last_digest_at` (already there) is the other boundary, "since the last
--    notification". The aggregates behind both are read from `paper_trades`, which is why
--    `pnl` and `fees` become real columns: summing a JSONB field per delivery is a scan and a
--    cast where an indexed aggregate is neither.
--
-- 2. A **filter**. `notify_open` / `notify_close` say which side of a round trip is worth a
--    message, `notify_tickers` narrows to one instrument (empty = every one). The decision is
--    stamped on the event as `notify`, not applied at send time: the in-app log stays complete
--    (it is the trader's record) while the channel only carries what was asked for.
--
-- 3. The fix: a **closed round trip is never pruned again**. The book was a projection of the
--    last run, so a trade falling out of the trailing window was deleted, which made "since
--    the beginning" lose its own history and made a widened window report old trades as new.
--    Closed rows are history and stay; only an *open* row the run no longer produces is
--    dropped, because that one is genuinely no longer held.

ALTER TABLE paper_sessions
    ADD COLUMN IF NOT EXISTS seeded_at             TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS notify_open           BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS notify_close          BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS notify_tickers        TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS digest_title_template TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS digest_template       TEXT NOT NULL DEFAULT '';

-- Sessions that already ran: their book was seeded at creation, near enough for the boundary.
UPDATE paper_sessions SET seeded_at = created_at WHERE seeded_at IS NULL AND inputs_hash <> '';

ALTER TABLE paper_trades
    ADD COLUMN IF NOT EXISTS pnl  DOUBLE PRECISION NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS fees DOUBLE PRECISION NOT NULL DEFAULT 0;

UPDATE paper_trades
   SET pnl  = COALESCE((trade ->> 'pnl')::double precision, 0),
       fees = COALESCE((trade ->> 'fees')::double precision, 0)
 WHERE pnl = 0 AND fees = 0;

-- The aggregate reads closed round trips of one session over a time span, in that order.
CREATE INDEX IF NOT EXISTS idx_paper_trades_closed
    ON paper_trades(session_id, closed_at) WHERE NOT open;

ALTER TABLE paper_events
    ADD COLUMN IF NOT EXISTS notify BOOLEAN NOT NULL DEFAULT true;

-- Only a deliverable event is ever picked up by a send; the rest stay in the log.
DROP INDEX IF EXISTS idx_paper_events_unsent;
CREATE INDEX IF NOT EXISTS idx_paper_events_unsent
    ON paper_events(session_id, at) WHERE notified_at IS NULL AND notify;
