-- Holes found in a dataset's candles (the market traded, nothing is stored), and the
-- download sent to fill each one. A hole whose download finished without closing it is
-- `confirmed`: the provider has no data there, so it is never asked for again and the
-- trades it cuts through stay excluded.
CREATE TABLE histdata_gaps (
    dataset_id  UUID NOT NULL REFERENCES histdata_datasets(id) ON DELETE CASCADE,
    gap_from    TIMESTAMPTZ NOT NULL,  -- last candle before the hole
    gap_to      TIMESTAMPTZ NOT NULL,  -- first candle after it
    job_id      UUID,
    status      TEXT NOT NULL DEFAULT 'filling',  -- filling | confirmed
    checked_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (dataset_id, gap_from, gap_to)
);
