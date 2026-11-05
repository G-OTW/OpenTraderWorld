-- TaxCalculator module.
--
-- Estimates trading/investing tax only (no personal/income tax). Two persisted objects:
-- a reusable Tax Profile (country + person type + rules as JSONB) and a per-year Scenario
-- (the actual inputs + cached computed result). Country rule templates are NOT a table —
-- they ship as static data in otw-core (taxcalc::templates) for easy versioning. The
-- engine is pure and lives in otw-core; the store only persists. Estimates, not advice.
-- Single-user: no owner scoping.

CREATE TABLE IF NOT EXISTS taxcalc_profiles (
    id                    UUID PRIMARY KEY,
    name                  TEXT NOT NULL,
    -- ISO 3166-1 alpha-2, e.g. "FR", "US". Drives which template the profile derives from.
    country               TEXT NOT NULL,
    -- Optional state/province/canton where rates differ (US, CH, CA, ES).
    region                TEXT,
    -- ISO 4217 reporting currency for this profile.
    currency              TEXT NOT NULL DEFAULT 'USD',
    -- 'individual' | 'professional'
    person_type           TEXT NOT NULL DEFAULT 'individual',
    -- Named ruleset key, e.g. "fr_pfu", "us_federal", "custom_flat".
    regime                TEXT NOT NULL DEFAULT 'custom_flat',
    -- For regimes taxing gains as income (percent, 0..100). NULL = use regime default.
    marginal_income_rate  DOUBLE PRECISION,
    -- e.g. FR prélèvements sociaux 17.2 (percent). NULL = use regime default.
    social_charges_rate   DOUBLE PRECISION,
    -- Per-income-type annual tax-free allowances.
    allowances            JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- { years: int, ring_fenced: bool }
    loss_carry            JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Holding-period relief table: [{ min_days, rate }] (investing long-term relief).
    holding_period_rules  JSONB NOT NULL DEFAULT '[]'::jsonb,
    -- Optional wealth/holding tax brackets on portfolio value: [{ up_to, rate }].
    wealth_tax            JSONB,
    notes                 TEXT NOT NULL DEFAULT '',
    -- True when built from a template and edited, or fully user-defined.
    is_custom             BOOLEAN NOT NULL DEFAULT FALSE,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS taxcalc_scenarios (
    id           UUID PRIMARY KEY,
    profile_id   UUID NOT NULL REFERENCES taxcalc_profiles(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    tax_year     INTEGER NOT NULL,
    -- 'summary' | 'itemized'
    mode         TEXT NOT NULL DEFAULT 'summary',
    -- 'trading' | 'investing'
    context      TEXT NOT NULL DEFAULT 'investing',
    -- ISO 4217 currency the inputs are entered in (FX-converted to profile currency).
    currency     TEXT NOT NULL DEFAULT 'USD',
    -- All numeric inputs (summary X/Y/contributions/withdrawals or itemized rows) as JSONB;
    -- shape depends on `mode`. The engine owns interpretation.
    inputs       JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Cached computed breakdown from the last compute (engine output). Recomputed on change.
    result       JSONB,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_taxcalc_scenarios_profile ON taxcalc_scenarios(profile_id);
