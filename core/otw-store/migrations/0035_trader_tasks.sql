-- Trader Tasks: recurring session routines (checklists ticked per trading day)
-- plus one-off quick tasks.

CREATE TABLE trader_routines (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    -- Which part of the trading day the routine belongs to: pre | live | post | any.
    session TEXT NOT NULL DEFAULT 'pre',
    -- Due-weekday bitmask: Mon=1, Tue=2, Wed=4, Thu=8, Fri=16, Sat=32, Sun=64.
    weekdays INT NOT NULL DEFAULT 31,
    position DOUBLE PRECISION NOT NULL DEFAULT 0,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE trader_routine_items (
    id UUID PRIMARY KEY,
    routine_id UUID NOT NULL REFERENCES trader_routines(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    position DOUBLE PRECISION NOT NULL DEFAULT 0
);
CREATE INDEX trader_routine_items_routine ON trader_routine_items(routine_id);

-- One row per (item, day) tick; unticking deletes the row.
CREATE TABLE trader_routine_checks (
    item_id UUID NOT NULL REFERENCES trader_routine_items(id) ON DELETE CASCADE,
    check_date DATE NOT NULL,
    checked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (item_id, check_date)
);

CREATE TABLE trader_tasks (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    -- low | normal | high
    priority TEXT NOT NULL DEFAULT 'normal',
    due_date DATE,
    done BOOLEAN NOT NULL DEFAULT FALSE,
    done_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
