-- Subscriptions: price history. One row per price a subscription had, from the date it
-- applied. Changing the price or the currency appends a row; the same price adds none.
CREATE TABLE IF NOT EXISTS subscription_prices (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subscription_id UUID NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    price           DOUBLE PRECISION NOT NULL,
    currency        TEXT NOT NULL,
    effective_from  DATE NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_subscription_prices_sub
    ON subscription_prices(subscription_id, effective_from);

-- Existing subscriptions start their history with the price they hold today.
INSERT INTO subscription_prices (subscription_id, price, currency, effective_from)
SELECT s.id, s.price, s.currency, COALESCE(s.started_on, s.created_at::date)
FROM subscriptions s
WHERE NOT EXISTS (SELECT 1 FROM subscription_prices p WHERE p.subscription_id = s.id);
