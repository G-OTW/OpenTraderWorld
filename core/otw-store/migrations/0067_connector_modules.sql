-- Centralized data broker: one connector list shared by every data module.
--
-- Connectors used to be namespaced by an owning module (`scope`, 0059/0060): the same
-- provider account — key included — had to be re-created in Historical Data and in
-- Watchlists. They are now global, with explicit per-module grants in `connector_modules`.
-- A row with module '*' grants every data module, present and future.

CREATE TABLE IF NOT EXISTS connector_modules (
    connector_id UUID NOT NULL REFERENCES histdata_connectors(id) ON DELETE CASCADE,
    module       TEXT NOT NULL,
    PRIMARY KEY (connector_id, module)
);

-- Existing connectors keep exactly the reach they had: their old owning module.
INSERT INTO connector_modules (connector_id, module)
SELECT id, scope FROM histdata_connectors
ON CONFLICT DO NOTHING;

-- Names go back to globally unique (0060 made them per-scope). Rename the collisions the
-- split namespaces allowed, picking the first free "<name> #n", before re-adding the
-- constraint — a duplicate would otherwise fail the migration on boot.
DO $$
DECLARE r RECORD; candidate TEXT; i INT;
BEGIN
    FOR r IN
        SELECT id, name FROM histdata_connectors c
        WHERE EXISTS (
            SELECT 1 FROM histdata_connectors o
            WHERE o.name = c.name AND (o.created_at, o.id) < (c.created_at, c.id)
        )
    LOOP
        i := 2;
        LOOP
            candidate := r.name || ' #' || i;
            EXIT WHEN NOT EXISTS (SELECT 1 FROM histdata_connectors WHERE name = candidate);
            i := i + 1;
        END LOOP;
        UPDATE histdata_connectors SET name = candidate WHERE id = r.id;
    END LOOP;
END $$;

ALTER TABLE histdata_connectors DROP CONSTRAINT IF EXISTS histdata_connectors_scope_name_key;
ALTER TABLE histdata_connectors ADD CONSTRAINT histdata_connectors_name_key UNIQUE (name);
ALTER TABLE histdata_connectors DROP COLUMN IF EXISTS scope;
