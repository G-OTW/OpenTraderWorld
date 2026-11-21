-- Market sessions, and how a chart alert watches and how often it may fire.

-- A session is a wall-clock window in a timezone, on some weekdays. Stored in the venue's own
-- timezone rather than in UTC so a DST switch moves it with the market instead of shifting
-- it by an hour twice a year. `end_minute <= start_minute` means the window runs past
-- midnight into the next day. `weekdays` is the day the window *opens*, Monday = bit 0.
CREATE TABLE IF NOT EXISTS market_sessions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name         TEXT        NOT NULL,
    timezone     TEXT        NOT NULL,
    start_minute INT         NOT NULL CHECK (start_minute BETWEEN 0 AND 1439),
    end_minute   INT         NOT NULL CHECK (end_minute BETWEEN 0 AND 1439),
    weekdays     INT         NOT NULL DEFAULT 31 CHECK (weekdays BETWEEN 1 AND 127),
    position     INT         NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The three cash sessions most traders mean. Editable and deletable like any other row.
INSERT INTO market_sessions (name, timezone, start_minute, end_minute, weekdays, position)
SELECT * FROM (VALUES
    ('Asia/Pacific', 'Asia/Tokyo',       540,  930, 31, 0),
    ('Europe',       'Europe/London',    480,  990, 31, 1),
    ('US',           'America/New_York', 570,  960, 31, 2)
) AS v(name, timezone, start_minute, end_minute, weekdays, position)
WHERE NOT EXISTS (SELECT 1 FROM market_sessions);

-- mode:
--   'app'             live feed, evaluated only while the app is open in a browser
--   'background'      closed bars, polled by the server with no browser open
--   'background_live' live feed held by the server with no browser open
-- frequency:
--   'once'      fire, then disable
--   'every'     every crossing
--   'bar_close' judged on closed bars only
--   'cooldown'  every crossing, at most once per cooldown_secs
--   'session'   at most once per occurrence of session_id, and only inside it
ALTER TABLE histviz_alerts
    ADD COLUMN IF NOT EXISTS mode       TEXT NOT NULL DEFAULT 'background'
        CHECK (mode IN ('app', 'background', 'background_live')),
    ADD COLUMN IF NOT EXISTS frequency  TEXT NOT NULL DEFAULT 'once'
        CHECK (frequency IN ('once', 'every', 'bar_close', 'cooldown', 'session')),
    ADD COLUMN IF NOT EXISTS session_id UUID REFERENCES market_sessions(id) ON DELETE SET NULL;

UPDATE histviz_alerts
   SET frequency = CASE
       WHEN NOT repeat THEN 'once'
       WHEN cooldown_secs > 0 THEN 'cooldown'
       ELSE 'every'
   END;

ALTER TABLE histviz_alerts DROP COLUMN IF EXISTS repeat;
