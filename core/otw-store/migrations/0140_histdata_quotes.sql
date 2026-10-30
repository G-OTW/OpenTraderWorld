-- Bid and ask OHLC next to the mid, for the providers that publish both sides (Capital.com,
-- OANDA). NULL everywhere else: `open..close` stays the series every reader uses, and the
-- backtest prices fills off these columns only when a run asks for bid/ask.
ALTER TABLE histdata_bars
    ADD COLUMN IF NOT EXISTS bid_open  NUMERIC,
    ADD COLUMN IF NOT EXISTS bid_high  NUMERIC,
    ADD COLUMN IF NOT EXISTS bid_low   NUMERIC,
    ADD COLUMN IF NOT EXISTS bid_close NUMERIC,
    ADD COLUMN IF NOT EXISTS ask_open  NUMERIC,
    ADD COLUMN IF NOT EXISTS ask_high  NUMERIC,
    ADD COLUMN IF NOT EXISTS ask_low   NUMERIC,
    ADD COLUMN IF NOT EXISTS ask_close NUMERIC;
