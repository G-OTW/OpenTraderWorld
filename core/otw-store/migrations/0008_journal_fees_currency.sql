-- Trading Journal: fee schedules, per-trade currency & unit type, and journal settings.
--
-- Three additions:
--   1. Unit type on a trade (lot | unit | contract | share) — describes what `quantity`
--      counts, and drives which fee-schedule rates apply.
--   2. Fee schedules — reusable fee templates the user defines once and applies to a
--      trade as a shortcut. A schedule has a rate that is either a fixed currency amount
--      or a percentage, charged per lot / per unit / per contract / per trade. Picking a
--      schedule auto-computes the trade's `fees` (overridable). `fee_schedule_id` records
--      which schedule was applied.
--   3. Currency per trade and per capital event (12 majors, validated in the API). No FX
--      conversion yet — values are stored in the currency the user entered. A journal-wide
--      "display currency" setting selects how the breakdown is labelled; a future FX feed
--      will convert into it.

-- ── Fee schedules ────────────────────────────────────────────────────────────
-- `amount_kind`: 'fixed' (a currency amount) | 'pct' (percent, applied to notional).
-- `per`: 'lot' | 'unit' | 'contract' | 'trade'. For 'trade' the amount is charged once;
-- otherwise it is charged amount × quantity (fixed) or amount% × notional (pct).
CREATE TABLE IF NOT EXISTS journal_fee_schedules (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL DEFAULT '',
    amount      DOUBLE PRECISION NOT NULL DEFAULT 0,
    amount_kind TEXT NOT NULL DEFAULT 'fixed' CHECK (amount_kind IN ('fixed', 'pct')),
    per         TEXT NOT NULL DEFAULT 'trade'
                CHECK (per IN ('lot', 'unit', 'contract', 'trade')),
    currency    TEXT NOT NULL DEFAULT 'USD',
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_journal_fee_schedules_pos ON journal_fee_schedules(position);

-- ── Trade: currency, unit type, applied fee schedule ─────────────────────────
ALTER TABLE journal_trades
    ADD COLUMN IF NOT EXISTS currency        TEXT NOT NULL DEFAULT 'USD',
    ADD COLUMN IF NOT EXISTS unit_type       TEXT NOT NULL DEFAULT 'unit'
        CHECK (unit_type IN ('lot', 'unit', 'contract', 'share')),
    ADD COLUMN IF NOT EXISTS fee_schedule_id UUID
        REFERENCES journal_fee_schedules(id) ON DELETE SET NULL;

-- ── Capital event: currency ──────────────────────────────────────────────────
ALTER TABLE journal_capital_events
    ADD COLUMN IF NOT EXISTS currency TEXT NOT NULL DEFAULT 'USD';

-- ── Journal settings ─────────────────────────────────────────────────────────
-- Single-row settings for the journal (single-user). `display_currency` selects the
-- currency the breakdown is presented in. No FX yet, so it is a display label; a future
-- daily FX feed will convert trade/capital values into it.
CREATE TABLE IF NOT EXISTS journal_settings (
    id               BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    display_currency TEXT NOT NULL DEFAULT 'USD',
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO journal_settings (id, display_currency)
SELECT TRUE, 'USD'
WHERE NOT EXISTS (SELECT 1 FROM journal_settings);
