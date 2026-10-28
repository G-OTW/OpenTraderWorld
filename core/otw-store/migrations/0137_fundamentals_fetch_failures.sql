-- A provider that refused a Fundamentals dataset for a subject (plan, quota, symbol not
-- covered), and when it may be asked again. Automatic refreshes skip it until then; a
-- manual refresh asks anyway. A success clears the row.
CREATE TABLE IF NOT EXISTS fund_fetch_failures (
    dataset   TEXT NOT NULL,
    subject   TEXT NOT NULL,
    provider  TEXT NOT NULL,
    error     TEXT NOT NULL,
    failed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    retry_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (dataset, subject, provider)
);
