-- Rate-limit-aware downloads: a job waits instead of failing, and can be cancelled.
--
-- Queueing several instruments at once (batch download) makes the provider's limits the
-- normal case rather than the exception. A job that runs into the connector's declared
-- quota, or into a 429, is now *parked*: status `waiting` plus the instant it may resume.
-- The worker claims it again then, from `chunk_cursor`, so nothing already downloaded is
-- refetched. `wait_count` grows with consecutive parks and drives the backoff when the
-- provider gives no Retry-After.
--
-- `batch_id` groups the jobs one submission queued, so the page can summarize them and
-- cancel what is left in one click. Cancelling a queued/waiting job is immediate; a
-- running one goes through `cancelling`, which the worker sees at its next chunk boundary
-- and turns into `cancelled`, keeping the bars already written.
--
-- status: queued | running | waiting | cancelling | done | partial | error | cancelled

ALTER TABLE histdata_jobs
    ADD COLUMN IF NOT EXISTS resume_at   TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS wait_reason TEXT,                     -- quota | rate_limit
    ADD COLUMN IF NOT EXISTS wait_count  INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS batch_id    UUID;

-- The worker's claim now also considers parked jobs whose resume time has come.
DROP INDEX IF EXISTS idx_histdata_jobs_pending;
CREATE INDEX IF NOT EXISTS idx_histdata_jobs_pending
    ON histdata_jobs(status, created_at)
    WHERE status IN ('queued', 'running', 'waiting', 'cancelling');

-- Batch summary/cancel reads every job of one submission.
CREATE INDEX IF NOT EXISTS idx_histdata_jobs_batch
    ON histdata_jobs(batch_id)
    WHERE batch_id IS NOT NULL;
