-- Trading Robots, second step: the tables take the module's name, a robot trade carries its own
-- P&L in exact decimals, the day's figures are decimals, and Robots becomes a data module.
--
-- Renamed with their indexes and constraints (the rows stay where they are):
--   paper_sessions -> robots, paper_trades -> robot_trades, paper_events -> robot_events,
--   paper_exec_instruments / paper_exec_orders / paper_exec_fills -> robot_exec_*,
--   paper_live_fills -> robot_live_fills.
-- The paper broker's own tables (paper_cash, paper_orders, paper_fills) keep their names: they
-- are the broker, not the robot.
DO $$
DECLARE
    pair TEXT[];
    r RECORD;
BEGIN
    FOREACH pair SLICE 1 IN ARRAY ARRAY[
        ARRAY['paper_sessions', 'robots'],
        ARRAY['paper_trades', 'robot_trades'],
        ARRAY['paper_events', 'robot_events'],
        ARRAY['paper_exec_instruments', 'robot_exec_instruments'],
        ARRAY['paper_exec_orders', 'robot_exec_orders'],
        ARRAY['paper_exec_fills', 'robot_exec_fills'],
        ARRAY['paper_live_fills', 'robot_live_fills']
    ] LOOP
        EXECUTE format('ALTER TABLE %I RENAME TO %I', pair[1], pair[2]);
        -- Constraints first: renaming a primary key or a unique constraint renames its index.
        FOR r IN
            SELECT conname FROM pg_constraint
            WHERE conrelid = pair[2]::regclass AND position(pair[1] IN conname) > 0
        LOOP
            EXECUTE format('ALTER TABLE %I RENAME CONSTRAINT %I TO %I', pair[2], r.conname, replace(r.conname, pair[1], pair[2]));
        END LOOP;
        FOR r IN
            SELECT c.relname FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid
            WHERE i.indrelid = pair[2]::regclass AND position(pair[1] IN c.relname) > 0
        LOOP
            EXECUTE format('ALTER INDEX %I RENAME TO %I', r.relname, replace(r.relname, pair[1], pair[2]));
        END LOOP;
    END LOOP;
END $$;

-- A robot trade's money, at its own size, in the instrument currency's precision. Written by
-- the app where the engine's closed trade becomes the robot's (and again when the replay
-- re-prices it); the rows already closed are filled here at 8 decimals, the app's figures
-- read them as they are.
ALTER TABLE robot_trades
    ADD COLUMN robot_pnl  NUMERIC,
    ADD COLUMN robot_fees NUMERIC;

UPDATE robot_trades
   SET robot_pnl  = round((pnl::numeric  * COALESCE(robot_qty / NULLIF(abs((trade->>'qty')::numeric), 0), 1)), 8),
       robot_fees = round((fees::numeric * COALESCE(robot_qty / NULLIF(abs((trade->>'qty')::numeric), 0), 1)), 8)
 WHERE NOT open;

-- The day's figures are sums of those decimals.
ALTER TABLE robot_daily
    ALTER COLUMN realized TYPE NUMERIC USING realized::numeric,
    ALTER COLUMN fees     TYPE NUMERIC USING fees::numeric;

-- Robots is a data module: a connector granted to Backtest serves Robots too, so no robot loses
-- its feed.
INSERT INTO connector_modules (connector_id, module)
SELECT connector_id, 'robots' FROM connector_modules WHERE module = 'backtest'
ON CONFLICT DO NOTHING;
