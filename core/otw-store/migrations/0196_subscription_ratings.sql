-- Subscriptions: usage ratings. The user rates a service 1 to 5 every quarter; the latest
-- rating feeds the "Worth it?" ranking.
CREATE TABLE IF NOT EXISTS subscription_ratings (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subscription_id UUID NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    rated_at        DATE NOT NULL DEFAULT CURRENT_DATE,
    score           SMALLINT NOT NULL CHECK (score BETWEEN 1 AND 5),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_subscription_ratings_sub
    ON subscription_ratings(subscription_id, rated_at DESC);
