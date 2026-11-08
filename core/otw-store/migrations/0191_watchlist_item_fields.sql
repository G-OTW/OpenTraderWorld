-- Watchlists: per-symbol fields the user sets on a row.
--
-- section     optional group inside a list (core, candidates, exits); NULL = ungrouped.
--             Rows keep their `position`; a drag between sections rewrites both.
-- flag        one colour from a fixed palette, NULL = no flag.
-- target_*    a buy and a sell price target, in the quote's currency.
-- virt_*      "if I had bought qty at price on date": a virtual position, nothing booked.
ALTER TABLE watchlist_items
    ADD COLUMN IF NOT EXISTS section     TEXT,
    ADD COLUMN IF NOT EXISTS flag        TEXT,
    ADD COLUMN IF NOT EXISTS target_buy  DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS target_sell DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS virt_qty    DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS virt_price  DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS virt_date   DATE;

ALTER TABLE watchlist_items DROP CONSTRAINT IF EXISTS watchlist_items_flag_check;
ALTER TABLE watchlist_items ADD CONSTRAINT watchlist_items_flag_check
    CHECK (flag IS NULL OR flag IN ('red', 'orange', 'yellow', 'green', 'blue', 'purple'));
