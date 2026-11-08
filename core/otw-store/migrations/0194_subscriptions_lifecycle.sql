-- Subscriptions: renewal reminders, free trials, cancellation, yearly price, payment
-- method, card expiry and shared plans.
--
-- `remind_days` null = no reminder. `remind_channels` is a JSON array of channel ids, `[]`
-- meaning every channel granted to Subscriptions. `reminded_for` is the billing date the
-- last reminder was sent for, so each billing date is reminded once.
-- `trial_end`: billing starts on that date. `ended_at`: the service is cancelled from that
-- date; the row and its history stay. `card_expiry` is `YYYY-MM`.
-- `my_share_pct`: the part of the price the user pays (family plans), 100 by default.
ALTER TABLE subscriptions
    ADD COLUMN IF NOT EXISTS remind_days     INTEGER CHECK (remind_days IS NULL OR remind_days BETWEEN 0 AND 60),
    ADD COLUMN IF NOT EXISTS remind_channels JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS reminded_for    DATE,
    ADD COLUMN IF NOT EXISTS trial_end       DATE,
    ADD COLUMN IF NOT EXISTS ended_at        DATE,
    ADD COLUMN IF NOT EXISTS yearly_price    DOUBLE PRECISION CHECK (yearly_price IS NULL OR yearly_price >= 0),
    ADD COLUMN IF NOT EXISTS payment_method  TEXT,
    ADD COLUMN IF NOT EXISTS card_expiry     TEXT CHECK (card_expiry IS NULL OR card_expiry ~ '^[0-9]{4}-(0[1-9]|1[0-2])$'),
    ADD COLUMN IF NOT EXISTS my_share_pct    DOUBLE PRECISION NOT NULL DEFAULT 100
        CHECK (my_share_pct > 0 AND my_share_pct <= 100);
