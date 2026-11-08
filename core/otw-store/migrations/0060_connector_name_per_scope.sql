-- Connector names are unique per owning module, not globally: with distinct Historical
-- Data and Watchlists namespaces (0059), "Coinbase" must be creatable in both.
ALTER TABLE histdata_connectors DROP CONSTRAINT IF EXISTS histdata_connectors_name_key;
ALTER TABLE histdata_connectors ADD CONSTRAINT histdata_connectors_scope_name_key UNIQUE (scope, name);
