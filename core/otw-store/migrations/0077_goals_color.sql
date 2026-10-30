-- Goals: per-goal accent colour.
--
-- Colours the card edge and its progress bar so goals are told apart at a glance.
-- NULL = no colour picked (the UI falls back to the default green/status colours).

ALTER TABLE goals ADD COLUMN IF NOT EXISTS color TEXT;
