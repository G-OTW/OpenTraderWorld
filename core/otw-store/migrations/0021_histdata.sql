-- Historical Data module ("histdata").
--
-- Downloads OHLCV from external providers into Postgres. A `datasets` catalog summarizes
-- each (provider, asset_type, ticker, timeframe) series; `histdata_bars` holds the rows.
-- Downloads run as resumable jobs drained by a single background worker. Provider
-- credentials are project-wide and encrypted with the same cipher as feed_secrets.
-- Single-user: no owner scoping.

-- Project-wide provider credentials (API key/secret). Plaintext is never stored or
-- returned: `ciphertext` is XChaCha20-Poly1305 AEAD output, `nonce` unique per secret.
-- The API exposes only (provider, name, is-set). Keyed by (provider, name) so a provider
-- can hold multiple named secrets, e.g. Alpaca's key + secret.
CREATE TABLE IF NOT EXISTS histdata_provider_creds (
    id         UUID PRIMARY KEY,
    provider   TEXT NOT NULL,           -- binance | coinbase | kraken | alphavantage | fmp | eodhd | yahoo | alpaca
    name       TEXT NOT NULL,           -- e.g. api_key | api_secret
    nonce      BYTEA NOT NULL,
    ciphertext BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, name)
);

-- One row per stored series. The management page reads this catalog directly and never
-- scans histdata_bars to summarize. `size_bytes`/`bar_count`/range are maintained as bars
-- are written. `gaps` is a JSON array of {from,to} UTC ranges with no data (failed chunk or
-- market-closed), so append/integrity checks are meaningful.
CREATE TABLE IF NOT EXISTS histdata_datasets (
    id           UUID PRIMARY KEY,
    provider     TEXT NOT NULL,
    asset_type   TEXT NOT NULL,         -- crypto | equity | etf | fx | index
    ticker       TEXT NOT NULL,         -- provider-native symbol (e.g. BTCUSDT, AAPL)
    timeframe    TEXT NOT NULL,         -- canonical: 1m|5m|15m|1h|4h|1d|1w
    range_from   TIMESTAMPTZ,           -- earliest stored bar ts (NULL until first bar)
    range_to     TIMESTAMPTZ,           -- latest stored bar ts
    bar_count    BIGINT NOT NULL DEFAULT 0,
    size_bytes   BIGINT NOT NULL DEFAULT 0,
    gaps         JSONB NOT NULL DEFAULT '[]'::jsonb,
    status       TEXT NOT NULL DEFAULT 'empty', -- empty | partial | complete | error
    last_updated TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, asset_type, ticker, timeframe)
);
-- Management page ordering: asset type -> ticker -> timeframe.
CREATE INDEX IF NOT EXISTS idx_histdata_datasets_sort
    ON histdata_datasets(asset_type, ticker, timeframe);

-- OHLCV rows. PK (dataset_id, ts) is the dedup/idempotency key on re-download.
-- Adjusted columns (splits/dividends) are populated for equities/ETFs only; NULL for
-- crypto/fx. Numeric for exact prices; DOUBLE PRECISION would lose precision on accumulation.
CREATE TABLE IF NOT EXISTS histdata_bars (
    dataset_id  UUID NOT NULL REFERENCES histdata_datasets(id) ON DELETE CASCADE,
    ts          TIMESTAMPTZ NOT NULL,  -- bar open time, UTC
    open        NUMERIC NOT NULL,
    high        NUMERIC NOT NULL,
    low         NUMERIC NOT NULL,
    close       NUMERIC NOT NULL,
    volume      NUMERIC NOT NULL DEFAULT 0,
    -- Split/dividend-adjusted OHLC (equities/ETFs). NULL where the provider/asset lacks it.
    adj_open    NUMERIC,
    adj_high    NUMERIC,
    adj_low     NUMERIC,
    adj_close   NUMERIC,
    PRIMARY KEY (dataset_id, ts)
);

-- Download jobs queue. A single background worker (histdata_job) drains queued jobs
-- serially, kind to provider rate limits. Each job is chunked and resumable: `chunk_cursor`
-- records the next ts to fetch so a failure at 80% resumes rather than restarting. There is
-- no separate validate step — a provider returning no data/error becomes status=error here.
CREATE TABLE IF NOT EXISTS histdata_jobs (
    id            UUID PRIMARY KEY,
    -- Target dataset (created/linked when the job is queued). NULL only if the row is
    -- orphaned by a dataset delete; ON DELETE SET NULL keeps job history readable.
    dataset_id    UUID REFERENCES histdata_datasets(id) ON DELETE SET NULL,
    -- Params snapshot so the job is self-describing even after the dataset changes.
    provider      TEXT NOT NULL,
    asset_type    TEXT NOT NULL,
    ticker        TEXT NOT NULL,
    timeframe     TEXT NOT NULL,
    range_from    TIMESTAMPTZ NOT NULL,
    range_to      TIMESTAMPTZ NOT NULL,
    kind          TEXT NOT NULL DEFAULT 'download', -- download | append (gap-fill)
    status        TEXT NOT NULL DEFAULT 'queued',   -- queued | running | done | partial | error
    -- Progress: chunks completed out of estimated total (total may be 0 until known).
    chunks_done   INTEGER NOT NULL DEFAULT 0,
    chunks_total  INTEGER NOT NULL DEFAULT 0,
    bars_written  BIGINT NOT NULL DEFAULT 0,
    -- Resume point: next bar ts to fetch (NULL = start from range_from).
    chunk_cursor  TIMESTAMPTZ,
    error         TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at    TIMESTAMPTZ,
    finished_at   TIMESTAMPTZ
);
-- Worker pulls the oldest queued/running job.
CREATE INDEX IF NOT EXISTS idx_histdata_jobs_pending
    ON histdata_jobs(status, created_at)
    WHERE status IN ('queued', 'running');
