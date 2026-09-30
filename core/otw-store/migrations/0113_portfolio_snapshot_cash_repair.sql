-- Portfolio Tracker: drop the cash balance from snapshots written for a ledger that has none.
--
-- A ledger holding no deposit and no withdrawal is not tracking a cash account. Earlier builds
-- debited every purchase against one anyway, so a snapshot written on such a day stored
-- `cash = -(everything ever spent)` and its net worth collapsed to the unrealized PnL. One such
-- row between two correct ones is a -86% day followed by a +630% one: it does not average out,
-- it dominates the drawdown, the volatility and the worst-day figure for good.
--
-- The condition is exact, not a threshold: it is the same test the code now applies before
-- booking anything into cash (`store::book`, `history::rebuild`). `flow` is deliberately left
-- alone; reconstructing it needs the ledger walk, which is what Rebuild is for.
UPDATE portfolio_snapshots s
SET cash = 0
WHERE s.cash <> 0
  AND NOT EXISTS (
      SELECT 1
      FROM portfolio_operations o
      LEFT JOIN portfolio_assets a ON a.id = o.asset_id
      WHERE COALESCE(o.portfolio_id, a.portfolio_id) = s.portfolio_id
        AND o.side IN ('deposit', 'withdraw')
  );
