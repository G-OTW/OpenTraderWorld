-- A robot ticks at each close of its strategy's timeframe. The recurrence columns (kind,
-- every_minutes, at_hour, ...) now plan its status report, sent when `cadence` is `scheduled`.
--
-- exclusions: `{ weekdays, hours: [{from, to}], dates: [{from, to}], leave_market }` in the
--   robot's timezone. No new entry inside one; `leave_market` closes what it holds on the way in.
-- excluded_at: when the robot entered the exclusion it is in now; NULL outside one.
-- next_report_at: the status report's next occurrence; NULL when none is planned.
ALTER TABLE robots
    ADD COLUMN exclusions JSONB NOT NULL DEFAULT '{}',
    ADD COLUMN excluded_at TIMESTAMPTZ,
    ADD COLUMN next_report_at TIMESTAMPTZ;

-- 0205 renamed `paper_sessions_cadence` to `robots_cadence`; either name may be the live one.
ALTER TABLE robots DROP CONSTRAINT IF EXISTS paper_sessions_cadence;
ALTER TABLE robots DROP CONSTRAINT IF EXISTS robots_cadence;
UPDATE robots SET cadence = 'scheduled' WHERE cadence IN ('hourly', 'daily', 'weekly');
ALTER TABLE robots ADD CONSTRAINT robots_cadence CHECK (cadence IN ('each', 'scheduled'));
