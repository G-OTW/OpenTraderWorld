-- The contract a futures trade was actually taken on.
--
-- A journal ticker is what the trader writes ("MNQ", "MNQU6", "MNQ Sep 26"); a provider
-- wants its own contract syntax (IBKR: MNQU6 or MNQ.202609@CME, Polygon: its vendor
-- prefix). For a stock the two are the same string and nothing is needed. For a future
-- they are not, and the root alone names no contract at all: downloading it would fetch
-- the continuous front month and measure a different instrument than the one traded.
--
-- So the journal asks instead of guessing: one entry per instrument, keyed
-- "<asset_class>:<TICKER>", value the symbol handed to the provider.
ALTER TABLE journal_settings
    ADD COLUMN IF NOT EXISTS market_symbols JSONB NOT NULL DEFAULT '{}'::jsonb;
