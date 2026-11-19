-- A broker robot's closed trade is priced from its own fills at the venue, fees included:
-- that figure is the robot's money, and a replay that re-prices the engine's trade keeps it.
ALTER TABLE robot_trades ADD COLUMN money_at_broker BOOLEAN NOT NULL DEFAULT false;

-- Two more things a sync can find: the broker did not answer several syncs in a row
-- (`unreachable`), and the user acted at the broker in a way the robot cannot follow, so it
-- was disarmed (`stopped`, until it is armed again).
ALTER TABLE robots DROP CONSTRAINT robots_exec_state;
ALTER TABLE robots ADD CONSTRAINT robots_exec_state
    CHECK (exec_state IN ('idle', 'synced', 'degraded', 'paused', 'blocked', 'error', 'unreachable', 'stopped'));
