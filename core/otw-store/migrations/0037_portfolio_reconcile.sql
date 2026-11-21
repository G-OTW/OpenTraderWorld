-- Portfolio Tracker: per-asset price-source override + reconciliation status.
--
-- An asset is added via symbol search and priced by its default provider (coingecko for crypto,
-- yahoo for stock/etf). This lets the user attach a *different* live-price source to an asset:
-- `spot_provider` (one of binance/kraken/coinbase/coingecko/yahoo) + a provider-specific ticker
-- typed by hand into `spot_symbol` (BTCUSDT vs BTC-USD vs XXBTZUSD differ per exchange). When
-- `spot_provider` is NULL the asset is priced by `provider`/`provider_id` as before.
--
-- Reconciliation records whether the chosen source actually returns a live price:
--   'ok'         — priced successfully, included in refresh.
--   'unresolved' — the source returned no price; excluded from auto-refresh until fixed.
--   'manual'     — user opted this asset out of auto-refresh (price maintained by hand).
-- The daily/auto refresh prices only 'ok' assets.

ALTER TABLE portfolio_assets
    ADD COLUMN IF NOT EXISTS spot_provider    TEXT,
    ADD COLUMN IF NOT EXISTS spot_symbol      TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS recon_status     TEXT NOT NULL DEFAULT 'ok',
    ADD COLUMN IF NOT EXISTS recon_checked_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS recon_note       TEXT NOT NULL DEFAULT '';

-- Allowed price sources for the override (NULL = use the default provider).
ALTER TABLE portfolio_assets DROP CONSTRAINT IF EXISTS portfolio_assets_spot_provider_check;
ALTER TABLE portfolio_assets ADD CONSTRAINT portfolio_assets_spot_provider_check
    CHECK (spot_provider IS NULL OR spot_provider IN
        ('coingecko', 'yahoo', 'binance', 'kraken', 'coinbase'));

ALTER TABLE portfolio_assets DROP CONSTRAINT IF EXISTS portfolio_assets_recon_status_check;
ALTER TABLE portfolio_assets ADD CONSTRAINT portfolio_assets_recon_status_check
    CHECK (recon_status IN ('ok', 'unresolved', 'manual'));
