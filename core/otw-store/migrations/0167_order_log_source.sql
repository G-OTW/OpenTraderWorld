-- Who sent an order: `session` (someone at the chart), `alert:<id>` (a trade on alert) or
-- `strategy:<id>` (a strategy on a broker account). NULL on rows written before it was kept,
-- and on reconciliation findings, which no sender caused.
ALTER TABLE broker_order_log ADD COLUMN source TEXT;
