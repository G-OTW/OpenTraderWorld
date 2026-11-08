-- Automator: the editor's unsaved draft.
--
-- `graph` is the working copy the engine runs, and it is validated on write: a block still
-- being filled in (an api block with no path yet) cannot go there without putting an
-- invalid graph in front of tonight's schedule. Autosave needs somewhere to put that work
-- anyway, so it lands here: never read by the engine, cleared the moment the graph saves
-- for real, and offered back to the editor on the next load.
ALTER TABLE automator_workflows ADD COLUMN IF NOT EXISTS draft JSONB;
