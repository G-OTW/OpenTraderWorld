-- Three messages, three templates: entry, exit, summary.
--
-- One pair of templates had to word both sides of a round trip, so a body naming
-- `trade.pnl` printed a blank on the entry and a body naming the entry price said
-- nothing about the exit. Each kind now owns its wording and, with it, its own
-- vocabulary: an entry resolves the entry half of the trade, an exit the whole of it,
-- and the summary talks about the period (`since.*`, `total.*`, `open.*`) with no trade
-- at all. The grouped message keeps its watermark (`last_digest_at`); only the wording
-- is renamed to what the user reads.

ALTER TABLE paper_sessions
    ADD COLUMN IF NOT EXISTS entry_title_template   TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS entry_template         TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS exit_title_template    TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS exit_template          TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS summary_title_template TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS summary_template       TEXT NOT NULL DEFAULT '';

-- The shared pair worded both sides, so it becomes both sides.
UPDATE paper_sessions SET
    entry_title_template   = title_template,
    entry_template         = body_template,
    exit_title_template    = title_template,
    exit_template          = body_template,
    summary_title_template = digest_title_template,
    summary_template       = digest_template;

ALTER TABLE paper_sessions
    DROP COLUMN IF EXISTS title_template,
    DROP COLUMN IF EXISTS body_template,
    DROP COLUMN IF EXISTS digest_title_template,
    DROP COLUMN IF EXISTS digest_template;
