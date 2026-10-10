-- Two fixes to the 0080 shape.
--
-- 1. The backfilled categories were named after the phases ("Before the session", "After
--    the session"), so the editor showed the same two labels twice — once as the phase
--    chips, once in the category list. A category answers *what kind of check-in*, a phase
--    answers *when in the day*; they are different axes and must not share a vocabulary.
--    Both fold into one "Daily" category, which is what they always were.
--
-- 2. `phase` gains `live` (during the session) alongside `pre` and `post` — a session
--    break is a real moment to check in, and the routines module already splits the day
--    three ways. No schema change: `phase` is a bare TEXT column, the API validates it.

-- Rename the first, move the second's templates into it, then drop the emptied one.
UPDATE mindset_categories SET name = 'Daily' WHERE name = 'Before the session';

UPDATE mindset_templates t
SET category_id = (SELECT id FROM mindset_categories WHERE name = 'Daily')
WHERE t.category_id = (SELECT id FROM mindset_categories WHERE name = 'After the session')
  AND EXISTS (SELECT 1 FROM mindset_categories WHERE name = 'Daily');

-- Only if nothing else ended up in it (a user may have filed their own templates there).
DELETE FROM mindset_categories c
WHERE c.name = 'After the session'
  AND NOT EXISTS (SELECT 1 FROM mindset_templates t WHERE t.category_id = c.id);
