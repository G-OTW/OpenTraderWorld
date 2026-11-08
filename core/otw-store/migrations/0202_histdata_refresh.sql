-- Scheduled refresh of a dataset: every `refresh_every_secs` (NULL = off, at least 60) the
-- trailing `refresh_window_bars` bars are pulled again through the dataset's provider and
-- upserted, so a stored series keeps up without a new download job.
--
-- An imported dataset can be refreshed too when it knows where its file lives: a URL
-- (`import_source_url`, fetched under the same outbound guard as a custom HTTP source) or a
-- path under the mounted import folder (`import_source_path`, relative to OTW_IMPORT_DIR).
-- `import_mapping` is what the last commit read the file with ({filename, mapping}), so a
-- re-import parses the new file exactly like the first one.

ALTER TABLE histdata_datasets
    ADD COLUMN IF NOT EXISTS refresh_every_secs  INTEGER,
    ADD COLUMN IF NOT EXISTS refresh_window_bars INTEGER NOT NULL DEFAULT 200,
    ADD COLUMN IF NOT EXISTS last_refresh_at     TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS last_refresh_error  TEXT,
    ADD COLUMN IF NOT EXISTS import_source_url   TEXT,
    ADD COLUMN IF NOT EXISTS import_source_path  TEXT,
    ADD COLUMN IF NOT EXISTS import_mapping      JSONB;

ALTER TABLE histdata_datasets
    DROP CONSTRAINT IF EXISTS histdata_datasets_refresh_every_check;
ALTER TABLE histdata_datasets
    ADD CONSTRAINT histdata_datasets_refresh_every_check
    CHECK (refresh_every_secs IS NULL OR refresh_every_secs >= 60);

CREATE INDEX IF NOT EXISTS idx_histdata_datasets_refresh
    ON histdata_datasets (last_refresh_at)
    WHERE refresh_every_secs IS NOT NULL;
