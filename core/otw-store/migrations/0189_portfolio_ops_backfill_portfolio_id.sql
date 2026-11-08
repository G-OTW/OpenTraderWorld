-- Rows seeded with only an asset_id are invisible to the ledger walk, which reads a whole
-- portfolio by portfolio_id. Give every such row its asset's portfolio. Idempotent.
UPDATE portfolio_operations SET portfolio_id = a.portfolio_id
FROM portfolio_assets a WHERE a.id = portfolio_operations.asset_id AND portfolio_operations.portfolio_id IS NULL;
