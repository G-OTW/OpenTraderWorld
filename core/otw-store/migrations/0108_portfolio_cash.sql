-- Portfolio Tracker: cash, income and costs in the ledger.
--
-- The ledger was buy/sell only, so a portfolio had no cash balance, no dividends and no
-- standalone fees: "net worth", "invested vs cash", "income" and "net performance after
-- fees" were not wrong, they were not computable. They all come from the same place, one
-- widened vocabulary on the ledger the module already keeps.
--
-- The column keeps the name `side`. Renaming it to `kind` would ripple through the store,
-- the API, the importer, the MCP schema and the frontend for one word, and every reader
-- already switches on it.
--
--   buy | sell                        move units, price is per unit
--   deposit | withdraw                external cash in/out, no units
--   dividend | interest | coupon      income, no units
--   fee | tax                         cost not attached to a trade
--
-- For a cash kind, `quantity` is 1 and `price` is the amount: one shape for every row, so
-- the ledger walk stays a single pass and nothing downstream needs a second code path.

ALTER TABLE portfolio_operations DROP CONSTRAINT IF EXISTS portfolio_operations_side_check;
ALTER TABLE portfolio_operations ADD CONSTRAINT portfolio_operations_side_check
    CHECK (side IN ('buy', 'sell', 'deposit', 'withdraw', 'dividend', 'interest', 'coupon', 'fee', 'tax'));

-- A deposit belongs to the portfolio, not to an asset. `portfolio_id` has been on the row
-- since 0085 (import de-duplication needed it), so a cash row is still addressable without
-- inventing a synthetic cash asset — which would have polluted the positions table, the
-- allocation donut and every per-asset query in the module.
ALTER TABLE portfolio_operations ALTER COLUMN asset_id DROP NOT NULL;

-- Currency of a cash row. NULL keeps today's meaning for an asset row: the amounts are in
-- the asset's own currency. Required when there is no asset to read it from.
ALTER TABLE portfolio_operations ADD COLUMN IF NOT EXISTS currency TEXT;

-- Backfill is a no-op by construction: every existing row is a buy or a sell with an asset.
UPDATE portfolio_operations SET portfolio_id = a.portfolio_id
FROM portfolio_assets a WHERE a.id = portfolio_operations.asset_id AND portfolio_operations.portfolio_id IS NULL;

ALTER TABLE portfolio_operations DROP CONSTRAINT IF EXISTS portfolio_operations_addressable;
ALTER TABLE portfolio_operations ADD CONSTRAINT portfolio_operations_addressable
    CHECK (asset_id IS NOT NULL OR (portfolio_id IS NOT NULL AND currency IS NOT NULL));

-- A trade without an asset is meaningless; income and costs may or may not name one.
ALTER TABLE portfolio_operations DROP CONSTRAINT IF EXISTS portfolio_operations_trade_asset;
ALTER TABLE portfolio_operations ADD CONSTRAINT portfolio_operations_trade_asset
    CHECK (side NOT IN ('buy', 'sell') OR asset_id IS NOT NULL);

-- The ledger walk reads a whole portfolio in one pass now (it has to, to know the cash),
-- replacing one query per asset.
CREATE INDEX IF NOT EXISTS idx_portfolio_ops_pf ON portfolio_operations(portfolio_id, op_date);
