-- Chart layout per dataset: what the visualization module was showing when you left it —
-- indicators, drawings, plot style. Keyed by dataset because that is what the user thinks
-- of as "this chart"; an instrument that was never stored has no row (and no layout).
CREATE TABLE IF NOT EXISTS histviz_layouts (
    dataset_id  UUID PRIMARY KEY REFERENCES histdata_datasets(id) ON DELETE CASCADE,
    layout      JSONB       NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
