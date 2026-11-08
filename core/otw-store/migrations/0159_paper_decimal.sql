-- Paper broker: prices, quantities, fees, rates and money are exact decimals, as a venue
-- books them. A binary float cannot hold 0.1 or 0.25 x 3 exactly, so a sum of fills drifted
-- off zero and a price off its tick.
--
-- Existing rows keep their value: a float reads as its 15 significant digits, rounded to 12
-- decimals (below that is float dust, never a price, size or amount anyone entered), with
-- trailing zeros dropped.

ALTER TABLE paper_cash
    ALTER COLUMN amount TYPE NUMERIC USING trim_scale(round(amount::numeric, 12));

ALTER TABLE paper_orders
    ALTER COLUMN multiplier   TYPE NUMERIC USING trim_scale(round(multiplier::numeric, 12)),
    ALTER COLUMN tick         TYPE NUMERIC USING trim_scale(round(tick::numeric, 12)),
    ALTER COLUMN qty          TYPE NUMERIC USING trim_scale(round(qty::numeric, 12)),
    ALTER COLUMN limit_price  TYPE NUMERIC USING trim_scale(round(limit_price::numeric, 12)),
    ALTER COLUMN stop_price   TYPE NUMERIC USING trim_scale(round(stop_price::numeric, 12)),
    ALTER COLUMN trail_amount TYPE NUMERIC USING trim_scale(round(trail_amount::numeric, 12)),
    ALTER COLUMN trail_pct    TYPE NUMERIC USING trim_scale(round(trail_pct::numeric, 12)),
    ALTER COLUMN trail_best   TYPE NUMERIC USING trim_scale(round(trail_best::numeric, 12));

ALTER TABLE paper_fills
    ALTER COLUMN qty        TYPE NUMERIC USING trim_scale(round(qty::numeric, 12)),
    ALTER COLUMN price      TYPE NUMERIC USING trim_scale(round(price::numeric, 12)),
    ALTER COLUMN fee        TYPE NUMERIC USING trim_scale(round(fee::numeric, 12)),
    ALTER COLUMN multiplier TYPE NUMERIC USING trim_scale(round(multiplier::numeric, 12)),
    ALTER COLUMN realized   TYPE NUMERIC USING trim_scale(round(realized::numeric, 12)),
    ALTER COLUMN fx         TYPE NUMERIC USING trim_scale(round(fx::numeric, 12));
