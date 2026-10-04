-- Symbol pickers (Company, Alternative data, ETF) list favourites first, then the most
-- recently opened. `opened_at` is bumped each time the user opens the symbol.
ALTER TABLE fund_companies ADD COLUMN IF NOT EXISTS opened_at TIMESTAMPTZ NOT NULL DEFAULT now();
UPDATE fund_companies SET opened_at = created_at;

-- ETFs the user opened. The fund data itself stays in fund_snapshots (dataset `etf`): a row
-- here is only written once that snapshot exists, so a mistyped ticker is never kept.
CREATE TABLE IF NOT EXISTS fund_etfs (
    ticker    TEXT PRIMARY KEY,
    followed  BOOLEAN NOT NULL DEFAULT false,
    opened_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ETFs already fetched become known ones.
INSERT INTO fund_etfs (ticker, opened_at)
SELECT subject, fetched_at FROM fund_snapshots WHERE dataset = 'etf'
ON CONFLICT (ticker) DO NOTHING;
