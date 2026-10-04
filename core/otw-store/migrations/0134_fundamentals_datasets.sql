-- Fundamentals, phase 3: what the aggregators answer about a subject (a company ticker,
-- an ETF, the market calendar), stored as the provider's answer at fetch time.
--
-- A snapshot is one dataset for one subject (`estimates` for `AAPL`, `etf` for `SPY`,
-- `calendar` for `_`), replaced whole on refresh. `data` is already normalised to the
-- module's shape, so a reader never needs to know which provider filled it.

CREATE TABLE IF NOT EXISTS fund_snapshots (
    subject    TEXT NOT NULL,
    dataset    TEXT NOT NULL,
    provider   TEXT NOT NULL,
    data       JSONB NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (subject, dataset)
);

-- Earnings-call transcripts are documents (kind 'transcript'); their speaker turns live
-- here so the reader can show who spoke and split prepared remarks from Q&A.
CREATE TABLE IF NOT EXISTS fund_transcript_segments (
    document_id UUID NOT NULL REFERENCES fund_documents(id) ON DELETE CASCADE,
    ordinal     INTEGER NOT NULL,
    speaker     TEXT NOT NULL,
    -- exec | analyst | operator | other
    role        TEXT NOT NULL,
    -- prepared | qa
    section     TEXT NOT NULL,
    text        TEXT NOT NULL,
    PRIMARY KEY (document_id, ordinal)
);
