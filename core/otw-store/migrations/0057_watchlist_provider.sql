-- Watchlists: optional per-list quote provider (a Historical Data connector).
--
-- Default (connector_id NULL) keeps the Portfolio Tracker reconciliation scheme
-- (CoinGecko coin id / Yahoo ticker). When the user pins a connector, quotes for the
-- whole list are fetched through it instead, and the refresh floor drops from 60 s to
-- 5 s — the user opts into faster cadences knowing their own API plan's limits.
ALTER TABLE watchlists
    ADD COLUMN IF NOT EXISTS connector_id UUID REFERENCES histdata_connectors(id) ON DELETE SET NULL;

-- Floor moves from 60 to 5: sub-minute cadences are only reachable through the API when a
-- custom connector is set (enforced in the handler); the DB keeps a sanity floor.
ALTER TABLE watchlists DROP CONSTRAINT IF EXISTS watchlists_refresh_secs_check;
ALTER TABLE watchlists ADD CONSTRAINT watchlists_refresh_secs_check CHECK (refresh_secs >= 5);

-- Per-item ticker override in the connector's own symbol format (e.g. BTCUSDT on Binance,
-- AAPL.US on EODHD). Empty = derive a best-guess from the item's symbol.
ALTER TABLE watchlist_items
    ADD COLUMN IF NOT EXISTS quote_ticker TEXT NOT NULL DEFAULT '';
