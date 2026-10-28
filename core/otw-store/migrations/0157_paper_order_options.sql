-- Paper broker orders take the options the chart's ticket offers when the broker declares
-- them: reduce-only, post-only, trailing stops (by amount or percent, with the best price seen
-- since they started), and the if-touched types (mit, lit) in `order_type`, fok in `tif`.
ALTER TABLE paper_orders
    ADD COLUMN reduce_only  BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN post_only    BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN trail_amount DOUBLE PRECISION CHECK (trail_amount > 0),
    ADD COLUMN trail_pct    DOUBLE PRECISION CHECK (trail_pct > 0 AND trail_pct < 100),
    ADD COLUMN trail_best   DOUBLE PRECISION;
