-- A future's initial margin per contract, in the instrument's currency, as the user states it:
-- what a paper account sets aside to hold one. `margin_notional` says to count the contract's
-- full value instead. Both null = not stated, and a paper order that would open or grow a
-- position on the contract is refused until one is.
ALTER TABLE instrument_meta
    ADD COLUMN IF NOT EXISTS margin DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS margin_notional BOOLEAN;
