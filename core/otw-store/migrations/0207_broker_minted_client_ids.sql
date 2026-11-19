-- Batch 2 venues: a client id OTW mints for a leg rather than spells from its key (Bitfinex: a
-- 45-bit integer with its date). Written before the order leaves, read back after a restart so
-- the leg can still be looked up, cancelled and matched on the account stream.
--
-- venue_client_id: the venue's client id as minted; NULL where the venue takes OTW's key.
-- venue_client_at: when it was minted, epoch milliseconds (the venue's clock).
ALTER TABLE broker_orders
    ADD COLUMN venue_client_id TEXT,
    ADD COLUMN venue_client_at BIGINT;
