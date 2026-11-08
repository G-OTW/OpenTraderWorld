-- Colour flag on a document (TradingView-style marker in the tree).
-- NULL = no flag; otherwise one of the named colours the UI offers.
ALTER TABLE documents ADD COLUMN IF NOT EXISTS flag TEXT;
