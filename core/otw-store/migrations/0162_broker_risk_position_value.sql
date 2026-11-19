-- A position limit by value, in the account's currency (size held once the order filled, times
-- the price, times a future's point value): one figure that means the same on every
-- instrument, where a size limit does not. Off (NULL) until set.
ALTER TABLE broker_risk_limits ADD COLUMN max_position_value NUMERIC CHECK (max_position_value > 0);
