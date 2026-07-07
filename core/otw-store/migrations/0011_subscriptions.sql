-- Subscription Tracker module.
--
-- Track recurring subscriptions (name, platform, price, currency, billing frequency,
-- category) and break down monthly spend. Money is converted into a per-module display
-- currency via the shared journal_fx rates, so figures stay coherent across currencies.
-- Single-user: no owner scoping.

-- ── Subscriptions ────────────────────────────────────────────────────────────
-- `frequency` is the billing cadence; the breakdown normalises each sub to a
-- monthly-equivalent (weekly ×52/12, monthly ×1, quarterly /3, yearly /12).
-- `started_on` anchors which calendar months a sub is actually billed (for the
-- "next month planned" figure); when null the sub is treated as always-active.
CREATE TABLE IF NOT EXISTS subscriptions (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL DEFAULT '',
    platform   TEXT,
    url        TEXT,
    price      DOUBLE PRECISION NOT NULL DEFAULT 0,
    currency   TEXT NOT NULL DEFAULT 'USD',
    frequency  TEXT NOT NULL DEFAULT 'monthly'
               CHECK (frequency IN ('weekly', 'monthly', 'quarterly', 'yearly')),
    category   TEXT,
    started_on DATE,
    active     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_subscriptions_active ON subscriptions(active);

-- ── Settings ─────────────────────────────────────────────────────────────────
-- Single-row module settings (display currency for the breakdown).
CREATE TABLE IF NOT EXISTS subscription_settings (
    id               BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    display_currency TEXT NOT NULL DEFAULT 'USD',
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO subscription_settings (id, display_currency)
SELECT TRUE, 'USD'
WHERE NOT EXISTS (SELECT 1 FROM subscription_settings);
