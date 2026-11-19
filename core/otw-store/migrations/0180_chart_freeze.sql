-- The charts' freeze on new entries: until `until`, an order sent from a chart that does not
-- reduce a position is refused, on every account. One row; a freeze started while one runs
-- keeps the later end. Nothing lifts it: it ends at `until`, server time.
CREATE TABLE chart_freeze (
    id         BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    until      TIMESTAMPTZ NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
