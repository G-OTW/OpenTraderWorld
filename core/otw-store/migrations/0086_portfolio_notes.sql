-- Portfolio Tracker — the long note behind a portfolio.
--
-- `description` is the one-liner on the card; this is the thesis: why this book exists,
-- what it is meant to do, the rules it follows. Free text, kept with the portfolio rather
-- than in a document, because it is read next to the holdings it justifies.
ALTER TABLE portfolios ADD COLUMN IF NOT EXISTS notes TEXT NOT NULL DEFAULT '';
