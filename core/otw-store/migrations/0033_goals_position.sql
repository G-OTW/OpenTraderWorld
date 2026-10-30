-- Goals: manual drag-to-reorder support.
--
-- Adds a `position` column so goals keep a user-defined order. Existing rows are
-- backfilled in their current display order (deadline first, then created_at) so the
-- visible order is preserved on first load.

ALTER TABLE goals ADD COLUMN IF NOT EXISTS position DOUBLE PRECISION NOT NULL DEFAULT 0;

WITH ordered AS (
    SELECT id, ROW_NUMBER() OVER (ORDER BY deadline NULLS LAST, created_at) AS rn
    FROM goals
)
UPDATE goals g SET position = o.rn
FROM ordered o WHERE g.id = o.id;

CREATE INDEX IF NOT EXISTS idx_goals_pos ON goals(position);
