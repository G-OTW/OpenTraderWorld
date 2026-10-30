-- Tax profiles: user-overridable flat rate on gains. NULL = use the regime template's
-- flat rate (previously the template value was the only source, which made the
-- custom_flat regime compute 0% on investing gains regardless of the profile's rates).
ALTER TABLE taxcalc_profiles ADD COLUMN flat_rate DOUBLE PRECISION;
