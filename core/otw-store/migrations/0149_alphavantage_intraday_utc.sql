-- Alpha Vantage writes its intraday series in New York's wall-clock time (the series'
-- "Time Zone" metadata, US/Eastern), and those stamps were stored as if they were UTC: every
-- intraday row sat 4 or 5 hours early. The connector now converts them; this moves the rows
-- already stored to the instant they really cover, so old and new downloads line up.
--
-- Daily rows are dates and do not move. Nothing is deleted: each row keeps its prices and
-- only its stamp changes. The move is exactly reversible, with the two zones swapped
-- ((ts AT TIME ZONE 'America/New_York') AT TIME ZONE 'UTC'): the series runs 04:00 to 20:00
-- New York time, so no stored stamp falls in the repeated autumn hour.

CREATE TEMP TABLE av_intraday ON COMMIT DROP AS
SELECT id FROM histdata_datasets
WHERE provider = 'alphavantage' AND timeframe IN ('1m', '5m', '15m', '1h');

-- Rows: copied out, removed, put back at their true instant (an in-place UPDATE could
-- collide with a neighbour's key midway through the statement).
CREATE TEMP TABLE av_bars ON COMMIT DROP AS
SELECT b.* FROM histdata_bars b WHERE b.dataset_id IN (SELECT id FROM av_intraday);

DELETE FROM histdata_bars WHERE dataset_id IN (SELECT id FROM av_intraday);

UPDATE av_bars SET ts = (ts AT TIME ZONE 'UTC') AT TIME ZONE 'America/New_York';

INSERT INTO histdata_bars SELECT * FROM av_bars;

-- Holes are bounded by candle stamps: they move with them.
CREATE TEMP TABLE av_gaps ON COMMIT DROP AS
SELECT g.* FROM histdata_gaps g WHERE g.dataset_id IN (SELECT id FROM av_intraday);

DELETE FROM histdata_gaps WHERE dataset_id IN (SELECT id FROM av_intraday);

UPDATE av_gaps SET
    gap_from = (gap_from AT TIME ZONE 'UTC') AT TIME ZONE 'America/New_York',
    gap_to   = (gap_to   AT TIME ZONE 'UTC') AT TIME ZONE 'America/New_York';

INSERT INTO histdata_gaps SELECT * FROM av_gaps;

-- The catalog's range follows its rows.
UPDATE histdata_datasets d SET range_from = s.lo, range_to = s.hi
FROM (SELECT dataset_id, min(ts) AS lo, max(ts) AS hi FROM histdata_bars
      WHERE dataset_id IN (SELECT id FROM av_intraday) GROUP BY dataset_id) s
WHERE d.id = s.dataset_id;

-- The lower-timeframe cache is a cache: dropped, it is fetched again when next needed.
DELETE FROM histdata_intrabar_bars WHERE provider = 'alphavantage' AND timeframe IN ('1m', '5m', '15m', '1h');
DELETE FROM histdata_intrabar_windows WHERE provider = 'alphavantage' AND timeframe IN ('1m', '5m', '15m', '1h');
