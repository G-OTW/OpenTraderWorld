-- Connectors get an owning module scope. Historical Data and Watchlists each manage their
-- own distinct list of provider accounts (same table/mechanics, separate namespaces):
-- a connector created in one module's UI never shows up in the other's.
ALTER TABLE histdata_connectors
    ADD COLUMN IF NOT EXISTS scope TEXT NOT NULL DEFAULT 'histdata'
    CHECK (scope IN ('histdata', 'watchlists'));
