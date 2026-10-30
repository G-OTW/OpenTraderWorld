-- Paper broker: a broker account run by OpenTraderWorld itself. Orders rest here and fill on
-- the live price of the chart they were placed from; nothing leaves the app.
--
-- The account's settings (currency, commissions, slippage, leverage, shorts) are its
-- `broker_accounts.config`, like any broker. These tables are its state: the cash moved in and
-- out by the user, the orders, and the fills, which are its execution history (what the
-- journal's broker import reads).

-- Money the user put in or took out. A reset clears the account and leaves one deposit.
CREATE TABLE paper_cash (
    id          BIGSERIAL PRIMARY KEY,
    account_id  UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- deposit | withdraw
    kind        TEXT NOT NULL CHECK (kind IN ('deposit', 'withdraw')),
    -- Always positive, in the account's currency.
    amount      DOUBLE PRECISION NOT NULL CHECK (amount > 0)
);

CREATE INDEX idx_paper_cash_account ON paper_cash (account_id, at);

CREATE TABLE paper_orders (
    id           UUID PRIMARY KEY,
    account_id   UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    -- The chart's own id for the order (the client order id).
    key          TEXT NOT NULL,
    -- Where the price it fills on comes from: the chart's market-data connector and
    -- instrument. No foreign key: a deleted connector leaves the order unable to fill, which
    -- the engine says, rather than deleting it.
    connector_id UUID NOT NULL,
    provider     TEXT NOT NULL,
    asset_type   TEXT NOT NULL,
    ticker       TEXT NOT NULL,
    -- Instrument facts at placement: what the fills are booked with.
    currency     TEXT NOT NULL,
    multiplier   DOUBLE PRECISION NOT NULL,
    tick         DOUBLE PRECISION,
    -- buy | sell
    side         TEXT NOT NULL,
    -- market | limit | stop | stop_limit
    order_type   TEXT NOT NULL,
    qty          DOUBLE PRECISION NOT NULL CHECK (qty > 0),
    limit_price  DOUBLE PRECISION,
    stop_price   DOUBLE PRECISION,
    -- day | gtc | gtd | ioc
    tif          TEXT NOT NULL,
    expires_at   TIMESTAMPTZ,
    -- A bracket exit: held until this order fills.
    parent_id    UUID REFERENCES paper_orders(id) ON DELETE CASCADE,
    oca_group    TEXT,
    -- held | working | filled | cancelled | rejected | expired
    status       TEXT NOT NULL,
    -- A stop-limit whose stop was reached: it now rests as a limit.
    triggered    BOOLEAN NOT NULL DEFAULT FALSE,
    -- Why it stopped working, when it was not filled or cancelled by the user.
    reason       TEXT,
    placed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_paper_orders_open ON paper_orders (account_id) WHERE status IN ('held', 'working');
CREATE INDEX idx_paper_orders_working ON paper_orders (provider, asset_type, ticker) WHERE status = 'working';

CREATE TABLE paper_fills (
    id           UUID PRIMARY KEY,
    account_id   UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    order_id     UUID NOT NULL,
    at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    connector_id UUID NOT NULL,
    provider     TEXT NOT NULL,
    asset_type   TEXT NOT NULL,
    ticker       TEXT NOT NULL,
    side         TEXT NOT NULL,
    qty          DOUBLE PRECISION NOT NULL CHECK (qty > 0),
    price        DOUBLE PRECISION NOT NULL,
    -- Commission, positive, in the instrument's currency.
    fee          DOUBLE PRECISION NOT NULL,
    currency     TEXT NOT NULL,
    multiplier   DOUBLE PRECISION NOT NULL,
    -- Result this fill closed (weighted-average cost), in the instrument's currency.
    realized     DOUBLE PRECISION NOT NULL DEFAULT 0,
    -- Instrument currency to account currency, at the fill: what `realized` and `fee` moved
    -- the cash by.
    fx           DOUBLE PRECISION NOT NULL
);

CREATE INDEX idx_paper_fills_account_at ON paper_fills (account_id, at);
