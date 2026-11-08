-- Portfolio Tracker: corporate actions and transfers between portfolios.
--
-- split / reverse_split   ratio_num / ratio_den new units per old unit, cost basis unchanged
-- spin_off                child_asset_id receives child_qty units and cost_share_pct of the
--                         parent's cost basis; the parent keeps its units
-- transfer_out / _in      units leave one portfolio and land in another with their cost
--                         basis, no realized result; transfer_id pairs both sides
--
-- None of these moves cash: `price` is the carried cost per unit on a transfer and zero on
-- a corporate action, so the cash walk reads every row the same way it always has.

ALTER TABLE portfolio_operations DROP CONSTRAINT IF EXISTS portfolio_operations_side_check;
ALTER TABLE portfolio_operations ADD CONSTRAINT portfolio_operations_side_check
    CHECK (side IN ('buy', 'sell', 'deposit', 'withdraw', 'dividend', 'interest', 'coupon', 'fee', 'tax',
                    'split', 'reverse_split', 'spin_off', 'transfer_out', 'transfer_in'));

ALTER TABLE portfolio_operations DROP CONSTRAINT IF EXISTS portfolio_operations_trade_asset;
ALTER TABLE portfolio_operations ADD CONSTRAINT portfolio_operations_trade_asset
    CHECK (side NOT IN ('buy', 'sell', 'split', 'reverse_split', 'spin_off', 'transfer_out', 'transfer_in')
           OR asset_id IS NOT NULL);

ALTER TABLE portfolio_operations
    ADD COLUMN IF NOT EXISTS ratio_num                DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS ratio_den                DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS child_asset_id           UUID REFERENCES portfolio_assets(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS child_qty                DOUBLE PRECISION,
    -- Share of the parent's cost basis that moves to the child, in percent (0..100).
    ADD COLUMN IF NOT EXISTS cost_share_pct           DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS counterpart_portfolio_id UUID REFERENCES portfolios(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS transfer_id              UUID,
    -- The day the units were first acquired, kept on a transfer_in so a moved lot keeps its
    -- holding age and its cost converts at the rate of the day it was paid.
    ADD COLUMN IF NOT EXISTS lot_date                 DATE;

ALTER TABLE portfolio_operations DROP CONSTRAINT IF EXISTS portfolio_operations_split_ratio;
ALTER TABLE portfolio_operations ADD CONSTRAINT portfolio_operations_split_ratio
    CHECK (side NOT IN ('split', 'reverse_split') OR (ratio_num > 0 AND ratio_den > 0));

ALTER TABLE portfolio_operations DROP CONSTRAINT IF EXISTS portfolio_operations_spin_off;
ALTER TABLE portfolio_operations ADD CONSTRAINT portfolio_operations_spin_off
    CHECK (side <> 'spin_off' OR (child_asset_id IS NOT NULL AND child_qty > 0
                                  AND cost_share_pct >= 0 AND cost_share_pct <= 100));

CREATE INDEX IF NOT EXISTS idx_portfolio_ops_child ON portfolio_operations(child_asset_id)
    WHERE child_asset_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_portfolio_ops_transfer ON portfolio_operations(transfer_id)
    WHERE transfer_id IS NOT NULL;
