-- Live trading per account (onboarding spec section 9): an account whose orders move real
-- money takes them only once the user turned live trading on for it, with the trading lock
-- code. NULL is off. Turning trading off for the account turns this off too.
ALTER TABLE broker_accounts ADD COLUMN live_since TIMESTAMPTZ;

-- A position that moved without fills to explain it halts its account until the user
-- acknowledges the drift (spec 7.2): only orders that reduce a position go out meanwhile.
ALTER TABLE trading_halts DROP CONSTRAINT trading_halts_reason_check;
ALTER TABLE trading_halts ADD CONSTRAINT trading_halts_reason_check
    CHECK (reason IN ('kill_switch', 'loss_limit', 'position_drift'));
