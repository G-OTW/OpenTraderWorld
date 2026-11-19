-- Trading Journal: daily FX rates so the breakdown can convert mixed-currency trades
-- and capital into a single display currency.
--
-- Rates are stored against a USD base (one row per date+quote). A cross-rate between two
-- non-USD currencies is derived as rate(USD→quote) / rate(USD→base) at read time. ECB /
-- Frankfurter publish on business days only, so a trade dated on a weekend/holiday uses
-- the most recent prior business day's rate (carry-forward), resolved in the store layer.
--
-- Source precedence: 'frankfurter' (primary, historical) > 'er-api' (backup, latest only)
-- > 'manual' (user-entered for dates no source could supply). When every source is offline
-- and a date is still uncovered, it is recorded in journal_fx_pending and surfaced to the
-- user as a pending task to fill in by hand.

-- ── FX rates ─────────────────────────────────────────────────────────────────
-- USD-based: rate is "1 USD = <rate> <quote>". USD itself is implicit (rate 1) and never
-- stored. `source` records provenance; a manual entry overwrites a fetched one for the
-- same date+quote (the user is authoritative).
CREATE TABLE IF NOT EXISTS journal_fx_rates (
    rate_date  DATE NOT NULL,
    base       TEXT NOT NULL DEFAULT 'USD',
    quote      TEXT NOT NULL,
    rate       DOUBLE PRECISION NOT NULL,
    source     TEXT NOT NULL DEFAULT 'frankfurter'
               CHECK (source IN ('frankfurter', 'er-api', 'manual')),
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (rate_date, base, quote)
);
CREATE INDEX IF NOT EXISTS idx_journal_fx_rates_date ON journal_fx_rates(rate_date);

-- ── Pending FX tasks ─────────────────────────────────────────────────────────
-- A date for which we could not obtain rates from any online source. Listed on the
-- "Pending tasks" page so the user can enter rates manually; resolving it (inserting
-- the manual rates) clears the row.
CREATE TABLE IF NOT EXISTS journal_fx_pending (
    pending_date DATE PRIMARY KEY,
    reason       TEXT NOT NULL DEFAULT 'fetch failed',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
