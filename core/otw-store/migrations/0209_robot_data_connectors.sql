-- The data source each instrument of a robot is read through, as picked when it was created
-- and changeable after: `{ "<dataset id>": "<connector id>" }`. An instrument missing here is
-- read through its provider's default source (robots from before this column).
ALTER TABLE robots ADD COLUMN data_connectors JSONB NOT NULL DEFAULT '{}';
