-- The loss limit as a percentage of the equity the period started with, instead of an amount
-- in the account currency.
ALTER TABLE broker_risk_limits ADD COLUMN max_loss_pct BOOLEAN NOT NULL DEFAULT FALSE;
