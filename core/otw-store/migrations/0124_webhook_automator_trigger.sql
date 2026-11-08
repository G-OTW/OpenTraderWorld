-- A webhook endpoint can now redirect its payload to a workflow, so a run may be started
-- by an inbound hook. The trigger list is a CHECK, not an enum: widen it.
--
-- Nothing else moves. The run is the same row a schedule produces, under the same
-- no-overlap and watchdog rules; only its provenance is new.

ALTER TABLE automator_runs DROP CONSTRAINT IF EXISTS automator_runs_trigger_check;
ALTER TABLE automator_runs
    ADD CONSTRAINT automator_runs_trigger_check
    CHECK (trigger IN ('manual', 'schedule', 'catchup', 'test', 'webhook'));
