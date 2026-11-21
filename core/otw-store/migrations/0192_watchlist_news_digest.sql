-- Watchlists: the news badge reset point and the daily movers digest.
--
-- news_seen_at    news items published after this count on the row badges; "mark read"
--                 moves it to now.
-- digest          'off' or 'daily': once a day, at or after `digest_hour` (UTC), the top
--                 movers of the list go to `digest_channels` (empty = every channel
--                 granted to Watchlists). `digest_last_at` is the once-a-day gate.
ALTER TABLE watchlists
    ADD COLUMN IF NOT EXISTS news_seen_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS digest          TEXT NOT NULL DEFAULT 'off',
    ADD COLUMN IF NOT EXISTS digest_hour     SMALLINT NOT NULL DEFAULT 21,
    ADD COLUMN IF NOT EXISTS digest_channels UUID[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS digest_last_at  TIMESTAMPTZ;

ALTER TABLE watchlists DROP CONSTRAINT IF EXISTS watchlists_digest_check;
ALTER TABLE watchlists ADD CONSTRAINT watchlists_digest_check
    CHECK (digest IN ('off', 'daily') AND digest_hour BETWEEN 0 AND 23);
