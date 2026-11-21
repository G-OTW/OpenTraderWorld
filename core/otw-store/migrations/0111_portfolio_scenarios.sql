-- Portfolio Tracker: stress scenarios and the betas they are measured with.
--
-- A scenario is either a **historical replay** (a named window whose realized daily paths are
-- applied to today's holdings, no model at all) or a **factor shock** (a set of legs, each a
-- real instrument and a move). Composites such as "recession" are factor scenarios with
-- several legs; they ship seeded and editable, and a builtin keeps its slug so it can be
-- restored rather than silently lost.

CREATE TABLE IF NOT EXISTS portfolio_scenarios (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL DEFAULT 'Untitled scenario',
    -- 'factor' | 'historical'
    kind       TEXT NOT NULL DEFAULT 'factor',
    -- Stable identity of a shipped scenario, so an update can find the row it seeded.
    slug       TEXT UNIQUE,
    builtin    BOOLEAN NOT NULL DEFAULT FALSE,
    -- factor: [{ "factor": "sp500", "shock_pct": -20 }, …]
    --         a rates leg carries its own duration: [{ "factor":"rates","shock_bps":200,"duration":7.5 }]
    -- historical: { "from": "2008-09-01", "to": "2009-03-09" }
    legs       JSONB NOT NULL DEFAULT '[]'::jsonb,
    note       TEXT NOT NULL DEFAULT '',
    position   DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE portfolio_scenarios DROP CONSTRAINT IF EXISTS portfolio_scenarios_kind_check;
ALTER TABLE portfolio_scenarios ADD CONSTRAINT portfolio_scenarios_kind_check
    CHECK (kind IN ('factor', 'historical'));

-- Measured sensitivities, cached against the bar series they were fitted on.
--
-- A cache, never a source of truth: a row whose dataset moved is re-fitted, and a row that
-- can no longer be measured is deleted rather than kept stale. `r2` is stored because it is
-- what decides whether the beta is used at all — a fit nobody can explain is not a number,
-- and an asset that fails the floor lands in `unexplained` instead of getting a beta of 1.
CREATE TABLE IF NOT EXISTS portfolio_betas (
    asset_id      UUID NOT NULL REFERENCES portfolio_assets(id) ON DELETE CASCADE,
    factor        TEXT NOT NULL,
    lookback_days INTEGER NOT NULL,
    beta          DOUBLE PRECISION NOT NULL,
    r2            DOUBLE PRECISION NOT NULL,
    rows          INTEGER NOT NULL,
    -- Latest bar timestamp the fit saw. A landed download moves it and forces a re-fit;
    -- nothing else does.
    watermark     TIMESTAMPTZ,
    computed_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (asset_id, factor, lookback_days)
);

-- ── Seeded scenarios ─────────────────────────────────────────────────────────
INSERT INTO portfolio_scenarios (id, name, kind, slug, builtin, legs, note, position)
VALUES
  (gen_random_uuid(), 'S&P 500 −20%',        'factor',     'sp500-20',    TRUE,
   '[{"factor":"sp500","shock_pct":-20}]'::jsonb, '', 1),
  (gen_random_uuid(), 'Nasdaq −30%',          'factor',     'nasdaq-30',   TRUE,
   '[{"factor":"nasdaq","shock_pct":-30}]'::jsonb, '', 2),
  (gen_random_uuid(), 'Rates +200 bps',       'factor',     'rates-200',   TRUE,
   '[{"factor":"rates","shock_bps":200,"duration":7.5}]'::jsonb, '', 3),
  (gen_random_uuid(), 'EUR/USD −10%',         'factor',     'eurusd-10',   TRUE,
   '[{"factor":"eurusd","shock_pct":-10}]'::jsonb, '', 4),
  (gen_random_uuid(), 'Oil +40%',             'factor',     'oil40',       TRUE,
   '[{"factor":"oil","shock_pct":40}]'::jsonb, '', 5),
  (gen_random_uuid(), 'Credit spreads widen', 'factor',     'credit',      TRUE,
   '[{"factor":"credit","shock_pct":-8},{"factor":"sp500","shock_pct":-10}]'::jsonb, '', 6),
  (gen_random_uuid(), 'Inflation spike',      'factor',     'inflation',   TRUE,
   '[{"factor":"rates","shock_bps":150,"duration":7.5},{"factor":"oil","shock_pct":35},{"factor":"sp500","shock_pct":-8}]'::jsonb, '', 7),
  (gen_random_uuid(), 'Recession',            'factor',     'recession',   TRUE,
   '[{"factor":"sp500","shock_pct":-25},{"factor":"credit","shock_pct":-12},{"factor":"oil","shock_pct":-30},{"factor":"rates","shock_bps":-150,"duration":7.5}]'::jsonb, '', 8),
  (gen_random_uuid(), '2008 crisis',          'historical', 'gfc-2008',    TRUE,
   '{"from":"2008-09-01","to":"2009-03-09"}'::jsonb, '', 9),
  (gen_random_uuid(), '2020 covid shock',     'historical', 'covid-2020',  TRUE,
   '{"from":"2020-02-19","to":"2020-03-23"}'::jsonb, '', 10),
  (gen_random_uuid(), '2022 rates year',      'historical', 'rates-2022',  TRUE,
   '{"from":"2022-01-03","to":"2022-10-12"}'::jsonb, '', 11),
  (gen_random_uuid(), '2018 Q4 selloff',      'historical', 'q4-2018',     TRUE,
   '{"from":"2018-10-01","to":"2018-12-24"}'::jsonb, '', 12),
  (gen_random_uuid(), '2021 crypto top',      'historical', 'crypto-2021', TRUE,
   '{"from":"2021-11-08","to":"2022-06-18"}'::jsonb, '', 13)
ON CONFLICT (slug) DO NOTHING;
