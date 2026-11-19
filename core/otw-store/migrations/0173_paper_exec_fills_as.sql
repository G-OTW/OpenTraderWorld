-- The broker's number a strategy order's fills are named by, where it is not the id the order
-- is followed by (Binance: `venue_id` is the symbol and our client id, the trade history names
-- Binance's order number). Read on the lookup, so the real fill price is matched to the order.
ALTER TABLE paper_exec_orders ADD COLUMN fills_as TEXT;
