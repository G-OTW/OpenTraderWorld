-- Per-item quote source. The list-level connector (0057) is only a default; mixed lists
-- (crypto + ETFs on a crypto-only exchange) need the choice per asset:
--   ''      → follow the list default (which silently skips unsupported asset classes),
--   'auto'  → force the default reconciliation providers (CoinGecko / Yahoo),
--   <uuid>  → a specific Historical Data connector for this item.
ALTER TABLE watchlist_items
    ADD COLUMN IF NOT EXISTS quote_source TEXT NOT NULL DEFAULT '';
