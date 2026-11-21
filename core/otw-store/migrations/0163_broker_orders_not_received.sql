-- A leg the broker never received waits for the user, who sends it again or dismisses it:
-- `not_received` until then, `resent` or `dismissed` after.
ALTER TABLE broker_orders DROP CONSTRAINT broker_orders_status_check;
ALTER TABLE broker_orders ADD CONSTRAINT broker_orders_status_check
    CHECK (status IN ('pending', 'unknown', 'working', 'filled', 'cancelled', 'rejected', 'expired', 'not_received', 'resent', 'dismissed'));
