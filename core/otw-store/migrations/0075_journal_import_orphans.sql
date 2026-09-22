-- Repair: un-strand trades whose import batch was dropped from the history.
--
-- Removing a batch from the history nulls its trades' import_batch_id (the FK is
-- ON DELETE SET NULL) but used to leave import_row_hash in place. Such a trade cannot be
-- reverted (no batch) and blocks its own source file from being re-imported (the hash
-- counts it as a duplicate) — a dead end with no way back.
--
-- `forget_batch` now clears the hash in the same transaction. This clears the ones that
-- were stranded before that fix. Only the internal dedup tag is touched: no trade is
-- deleted, no field a user typed is changed.
UPDATE journal_trades
SET import_row_hash = NULL
WHERE import_batch_id IS NULL
  AND import_row_hash IS NOT NULL;
