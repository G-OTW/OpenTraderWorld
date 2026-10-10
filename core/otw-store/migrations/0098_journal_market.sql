-- Market-data enrichment for the Trading Journal.
--
-- The journal knows what the trader did; the candles know what price did while the
-- position was open. Joining the two is what turns "I lost on that one" into "the stop
-- sat inside the noise", and it is the only way MAE, MFE and exit efficiency exist at
-- all: none of them can be derived from a trade log.
--
-- Nothing here fetches anything. Bars come from the existing histdata catalog through
-- the existing download queue, so the journal adds no provider code and no new endpoint
-- on that side. What it adds is a per-trade row of measurements and the settings that
-- say which connector to draw them from.

-- Which connector serves which histdata asset type ({"crypto": "<uuid>", ...}); an empty
-- object means "pick, among the connectors granted to the journal, the first that
-- supports the asset type". Timeframe 'auto' derives the grain from how long the trades
-- are actually held. Sync mode: off = never queue anything, manual = only on the user's
-- click, auto = a new trade queues its own missing window.
ALTER TABLE journal_settings
    ADD COLUMN IF NOT EXISTS market_connectors JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS market_timeframe  TEXT   NOT NULL DEFAULT 'auto',
    ADD COLUMN IF NOT EXISTS market_sync_mode  TEXT   NOT NULL DEFAULT 'manual';

DO $$
BEGIN
    ALTER TABLE journal_settings
        ADD CONSTRAINT journal_settings_market_sync_mode_chk
        CHECK (market_sync_mode IN ('off', 'manual', 'auto'));
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- One row per trade that has been measured against bars. Dropped and rebuilt by a
-- recompute, so nothing here is a source of truth: it is a cache of a pure function of
-- (trade, bars). `bars` records how many candles the window actually covered, which is
-- what tells a thin measurement from a solid one.
CREATE TABLE IF NOT EXISTS journal_trade_metrics (
    trade_id          UUID PRIMARY KEY REFERENCES journal_trades(id) ON DELETE CASCADE,
    dataset_id        UUID,
    timeframe         TEXT NOT NULL,
    bars              INTEGER NOT NULL DEFAULT 0,

    -- Excursions, in the trade's own currency and in price terms. MAE is reported
    -- positive: it is how much the position was down at its worst.
    mae               DOUBLE PRECISION,
    mfe               DOUBLE PRECISION,
    mae_price         DOUBLE PRECISION,
    mfe_price         DOUBLE PRECISION,
    -- Same, in units of the planned risk, when the trade carried a stop.
    mae_r             DOUBLE PRECISION,
    mfe_r             DOUBLE PRECISION,
    -- Captured / MFE: what share of the best available result the exit kept.
    exit_efficiency   DOUBLE PRECISION,
    -- MFE minus what was captured: the money left on the table.
    giveback          DOUBLE PRECISION,
    time_to_mae_min   BIGINT,
    time_to_mfe_min   BIGINT,

    -- Context at entry: ATR over the bars before the entry, realized volatility
    -- (annualized, percent), and the two buckets they place the trade in.
    atr_entry         DOUBLE PRECISION,
    vol_pct           DOUBLE PRECISION,
    regime            TEXT,
    trend             TEXT,
    -- Stop geometry: how far the planned stop sat in ATR, and how close price came to it.
    stop_distance_atr DOUBLE PRECISION,
    stop_hit          BOOLEAN,

    computed_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_journal_trade_metrics_regime
    ON journal_trade_metrics(regime);
