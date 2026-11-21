-- Fundamentals: macro series and company data pulled from third-party providers through
-- the data broker (FRED, US Treasury, SEC EDGAR to start).
--
-- A row is a period: an observation or a statement is keyed by the start of the period it
-- covers, never by the instant it was fetched. Transforms (YoY, index 100, unit scaling)
-- are computed on read and never stored. Only the latest vintage is kept.
--
-- `status` is the last refresh: 'pending' while a fetch runs, 'ok', or 'error' with the
-- provider's own message in `error` (it names the fix).

CREATE TABLE IF NOT EXISTS fund_series (
    id             UUID PRIMARY KEY,
    provider       TEXT NOT NULL,
    code           TEXT NOT NULL,
    title          TEXT NOT NULL,
    category       TEXT NOT NULL DEFAULT 'other',
    country        TEXT NOT NULL DEFAULT '',
    unit           TEXT NOT NULL DEFAULT '',
    -- D | W | M | Q | A
    frequency      TEXT NOT NULL DEFAULT '',
    seasonal_adj   BOOLEAN NOT NULL DEFAULT false,
    notes          TEXT NOT NULL DEFAULT '',
    status         TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'ok', 'error')),
    error          TEXT,
    last_refreshed TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, code)
);

CREATE TABLE IF NOT EXISTS fund_observations (
    series_id    UUID NOT NULL REFERENCES fund_series(id) ON DELETE CASCADE,
    period_start DATE NOT NULL,
    value        DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (series_id, period_start)
);

CREATE TABLE IF NOT EXISTS fund_companies (
    id              UUID PRIMARY KEY,
    -- Same ticker convention as histdata (upper case, provider-free).
    ticker          TEXT NOT NULL UNIQUE,
    cik             TEXT NOT NULL,
    name            TEXT NOT NULL,
    exchange        TEXT NOT NULL DEFAULT '',
    currency        TEXT NOT NULL DEFAULT 'USD',
    country         TEXT NOT NULL DEFAULT '',
    sector          TEXT NOT NULL DEFAULT '',
    industry        TEXT NOT NULL DEFAULT '',
    sic             TEXT NOT NULL DEFAULT '',
    -- MMDD as EDGAR reports it.
    fiscal_year_end TEXT NOT NULL DEFAULT '',
    website         TEXT NOT NULL DEFAULT '',
    description     TEXT NOT NULL DEFAULT '',
    shares_out      DOUBLE PRECISION,
    followed        BOOLEAN NOT NULL DEFAULT false,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'ok', 'error')),
    error           TEXT,
    last_refreshed  TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One row per (company, kind, period, source). `lines` holds standardised keys
-- (revenue, net_income, ...) with the as-reported tag of each under `tags`.
CREATE TABLE IF NOT EXISTS fund_statements (
    company_id     UUID NOT NULL REFERENCES fund_companies(id) ON DELETE CASCADE,
    kind           TEXT NOT NULL CHECK (kind IN ('income', 'balance', 'cashflow')),
    -- FY | Q1 | Q2 | Q3 | Q4
    fiscal_period  TEXT NOT NULL,
    fiscal_year    INTEGER NOT NULL,
    period_start   DATE NOT NULL,
    period_end     DATE NOT NULL,
    source         TEXT NOT NULL,
    lines          JSONB NOT NULL,
    tags           JSONB NOT NULL DEFAULT '{}',
    PRIMARY KEY (company_id, kind, period_start, period_end, source)
);

CREATE TABLE IF NOT EXISTS fund_documents (
    id          UUID PRIMARY KEY,
    company_id  UUID NOT NULL REFERENCES fund_companies(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('filing', 'transcript')),
    form        TEXT NOT NULL DEFAULT '',
    -- Provider-unique id (an EDGAR accession number).
    accession   TEXT NOT NULL,
    filed_at    DATE NOT NULL,
    period      DATE,
    title       TEXT NOT NULL,
    url         TEXT NOT NULL DEFAULT '',
    source      TEXT NOT NULL,
    -- Fetched on demand; NULL until then.
    body        TEXT,
    search      TSVECTOR GENERATED ALWAYS AS (
                    to_tsvector('english', title || ' ' || coalesce(body, ''))
                ) STORED,
    UNIQUE (source, accession)
);

CREATE INDEX IF NOT EXISTS fund_documents_company_idx ON fund_documents (company_id, filed_at DESC);
CREATE INDEX IF NOT EXISTS fund_documents_search_idx ON fund_documents USING GIN (search);

-- Insider transactions parsed from Form 4. Kept across refreshes (a filing never changes),
-- one row per transaction line of a filing.
CREATE TABLE IF NOT EXISTS fund_insider_trades (
    company_id  UUID NOT NULL REFERENCES fund_companies(id) ON DELETE CASCADE,
    accession   TEXT NOT NULL,
    ordinal     INTEGER NOT NULL,
    filed_at    DATE NOT NULL,
    tx_date     DATE NOT NULL,
    insider     TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT '',
    -- SEC transaction code: P purchase, S sale, A grant, M option exercise, ...
    code        TEXT NOT NULL,
    acquired    BOOLEAN NOT NULL,
    shares      DOUBLE PRECISION NOT NULL,
    price       DOUBLE PRECISION,
    owned_after DOUBLE PRECISION,
    PRIMARY KEY (company_id, accession, ordinal)
);

CREATE INDEX IF NOT EXISTS fund_insider_trades_date_idx ON fund_insider_trades (company_id, tx_date DESC);
