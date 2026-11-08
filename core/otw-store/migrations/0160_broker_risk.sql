-- The user's own risk limits on trading accounts, and the halts that stop new orders
-- (onboarding spec section 5). Every limit is off (NULL) until the user sets it. A limit is a
-- user setting with a visible refusal, never a market rule.
CREATE TABLE broker_risk_limits (
    account_id          UUID PRIMARY KEY REFERENCES broker_accounts(id) ON DELETE CASCADE,
    -- Per order: size, and value at its price (times the point value of a future).
    max_order_qty       NUMERIC CHECK (max_order_qty > 0),
    max_order_value     NUMERIC CHECK (max_order_value > 0),
    -- Size held on one instrument once the order filled.
    max_position_qty    NUMERIC CHECK (max_position_qty > 0),
    -- A limit or stop no further than this from the chart's last price, in percent.
    collar_pct          NUMERIC CHECK (collar_pct > 0 AND collar_pct < 100),
    max_orders_per_min  INTEGER CHECK (max_orders_per_min > 0),
    max_open_orders     INTEGER CHECK (max_open_orders > 0),
    -- Loss over the current period (the halt policy's hour, day or week), in the account's
    -- currency: reached, the account halts.
    max_loss            NUMERIC CHECK (max_loss > 0),
    -- The same order sent again within this many seconds asks for a confirmation.
    duplicate_secs      INTEGER CHECK (duplicate_secs > 0),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- How a halt ends, for every account: at the start of the next hour, day or week in
-- `timezone`, or only when lifted by hand; and whether the password lifts it sooner.
CREATE TABLE trading_halt_policy (
    id            BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    reset         TEXT NOT NULL DEFAULT 'day' CHECK (reset IN ('hour', 'day', 'week', 'manual')),
    password_lift BOOLEAN NOT NULL DEFAULT TRUE,
    timezone      TEXT NOT NULL DEFAULT 'UTC',
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO trading_halt_policy DEFAULT VALUES;

-- A halt: new orders refused (risk-reducing ones still pass) on one account, or on every
-- account when `account_id` is NULL (the global kill switch).
CREATE TABLE trading_halts (
    id          BIGSERIAL PRIMARY KEY,
    account_id  UUID REFERENCES broker_accounts(id) ON DELETE CASCADE,
    reason      TEXT NOT NULL CHECK (reason IN ('kill_switch', 'loss_limit')),
    detail      TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- When it ends on its own; NULL: only when lifted.
    until       TIMESTAMPTZ,
    lifted_at   TIMESTAMPTZ
);
CREATE INDEX idx_trading_halts_open ON trading_halts (account_id) WHERE lifted_at IS NULL;

-- Each account's equity when the current loss period started: the loss is measured from it.
CREATE TABLE broker_equity_marks (
    account_id    UUID PRIMARY KEY REFERENCES broker_accounts(id) ON DELETE CASCADE,
    period_start  TIMESTAMPTZ NOT NULL,
    equity        NUMERIC NOT NULL,
    currency      TEXT NOT NULL
);
