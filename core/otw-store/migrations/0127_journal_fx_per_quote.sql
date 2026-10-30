-- Trading Journal FX: a pending task is a (date, currency) pair, not a date.
--
-- The old table keyed on the date alone, which could not express "USDT has no rate on
-- 2025-03-16": the breakdown flagged the date, the user filled the eleven majors that were
-- already stored, and the next read flagged the same date again. Keying on the currency
-- makes the task resolvable, and lets any currency a user actually trades (USDT, USDC, a
-- crypto quote asset) hold a rate without being on a hardcoded majors list.
--
-- Rates may now also come from a stored histdata series (source 'histdata'): a 1d close on
-- USD<quote> or <quote>USD priced by a real market. When no market exists, the pair stays
-- pending and only manual entry resolves it. Nothing is ever guessed.

ALTER TABLE journal_fx_pending ADD COLUMN IF NOT EXISTS quote TEXT NOT NULL DEFAULT '';
-- Date-only rows carry no currency and cannot be mapped onto the new key: the job and the
-- breakdown recreate whatever is still genuinely missing on the next read.
DELETE FROM journal_fx_pending WHERE quote = '';
ALTER TABLE journal_fx_pending ALTER COLUMN quote DROP DEFAULT;
ALTER TABLE journal_fx_pending DROP CONSTRAINT IF EXISTS journal_fx_pending_pkey;
ALTER TABLE journal_fx_pending ADD PRIMARY KEY (pending_date, quote);

ALTER TABLE journal_fx_rates DROP CONSTRAINT IF EXISTS journal_fx_rates_source_check;
ALTER TABLE journal_fx_rates ADD CONSTRAINT journal_fx_rates_source_check
    CHECK (source IN ('frankfurter', 'er-api', 'manual', 'histdata'));
