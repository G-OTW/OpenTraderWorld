-- Portfolio Tracker: per-asset trade currency.
--
-- Operations (price, fee) were implicitly USD: the ledger summed them raw and converted the
-- USD total to the portfolio's display currency at read time. That forced the user to convert
-- by hand when buying an asset quoted in another currency (a EUR-denominated ETF, a LSE line),
-- while the form's "Price (USD)" label sat next to a EUR portfolio.
--
-- Each asset now carries the currency its operations are entered in. Spot prices stay USD
-- (both providers quote USD) — this is about the *ledger*, not the quote. Cost basis and
-- realized PnL convert from the asset's currency at each operation's own date, so an old buy
-- keeps its historical rate rather than being re-valued at today's.
--
-- Backfill is 'USD', which is exactly what existing rows meant, so their figures are unchanged.

ALTER TABLE portfolio_assets
  ADD COLUMN IF NOT EXISTS currency TEXT NOT NULL DEFAULT 'USD';
