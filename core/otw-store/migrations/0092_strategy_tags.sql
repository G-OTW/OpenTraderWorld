-- Tags on saved strategies.
--
-- The backtester's strategy library is now searched from the page itself (one search box over
-- names *and* tags), so a strategy carries free-form tags like the prompt store does.

ALTER TABLE backtest_strategies
    ADD COLUMN IF NOT EXISTS tags TEXT[] NOT NULL DEFAULT '{}';
