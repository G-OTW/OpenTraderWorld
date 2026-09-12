-- Favorites survive a catalog re-import.
--
-- The catalog is a snapshot: reinstalling it TRUNCATEs findb_instruments, and the favorites
-- FK (ON DELETE CASCADE) took every saved instrument down with it — so updating the dataset
-- silently wiped the user's favorites. Favorites are now keyed by their natural identity
-- (asset_type, symbol); `instrument_id` becomes a nullable cache re-resolved after each
-- import, NULL meaning "not in the current snapshot" (delisted, or renamed upstream).

ALTER TABLE findb_favorites
    ADD COLUMN IF NOT EXISTS asset_type TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS symbol     TEXT NOT NULL DEFAULT '';

UPDATE findb_favorites f
   SET asset_type = i.asset_type, symbol = i.symbol
  FROM findb_instruments i
 WHERE i.id = f.instrument_id AND f.symbol = '';

-- A favorite whose instrument is already gone cannot be identified — nothing to keep.
DELETE FROM findb_favorites WHERE symbol = '';

-- Two catalog rows may share (asset_type, symbol); collapse such favorites onto the oldest.
DELETE FROM findb_favorites a
 USING findb_favorites b
 WHERE a.asset_type = b.asset_type
   AND a.symbol = b.symbol
   AND (a.created_at, a.id) > (b.created_at, b.id);

ALTER TABLE findb_favorites
    DROP CONSTRAINT IF EXISTS findb_favorites_instrument_id_fkey,
    DROP CONSTRAINT IF EXISTS findb_favorites_instrument_id_key,
    ALTER COLUMN instrument_id DROP NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_findb_fav_key ON findb_favorites(asset_type, symbol);
