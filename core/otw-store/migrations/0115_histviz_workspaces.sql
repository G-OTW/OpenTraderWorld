-- Histviz v2: a chart workspace is a grid of panes, and a layout belongs to an instrument
-- rather than to a stored dataset.
--
-- Two changes, one migration:
--
-- 1. `histviz_instrument_layouts` keys the per-instrument layout (indicators, drawings, plot
--    style) on the instrument's own coordinates instead of a dataset id. Charting is
--    coordinate-addressed everywhere else already (`/api/histviz/series`, the live hub); the
--    dataset key was the last place where looking at a symbol you never saved meant losing
--    everything you drew on it. Existing rows are carried over by joining the dataset they
--    pointed at, then the old table is dropped: it cannot be kept in sync with the new one,
--    and a layout in two places is a layout that disagrees with itself.
--
-- 2. `histviz_workspaces` holds the grid: rows x columns, the pane in each cell, the splitter
--    fractions and the link groups. The pane keeps its own coordinates and may override the
--    instrument layout; a pane with no override falls back to `histviz_instrument_layouts`,
--    so an instrument still reopens the way you left it whichever pane it lands in.

CREATE TABLE IF NOT EXISTS histviz_instrument_layouts (
    -- 'provider|asset_type|ticker|timeframe', lowercased provider/asset/timeframe, the
    -- ticker kept verbatim (case matters to a provider, and only to the provider).
    coord_key   TEXT PRIMARY KEY,
    layout      JSONB       NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO histviz_instrument_layouts (coord_key, layout, updated_at)
SELECT lower(d.provider) || '|' || lower(d.asset_type) || '|' || d.ticker || '|' || lower(d.timeframe),
       l.layout,
       l.updated_at
FROM histviz_layouts l
JOIN histdata_datasets d ON d.id = l.dataset_id
ON CONFLICT (coord_key) DO NOTHING;

DROP TABLE IF EXISTS histviz_layouts;

CREATE TABLE IF NOT EXISTS histviz_workspaces (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT        NOT NULL,
    -- Grid shape. Free rows x columns (1x1 up to 3x4), so a vertical split, a horizontal
    -- split and a 2x2 are the same model rather than three named layouts.
    grid_rows   INT         NOT NULL DEFAULT 1,
    grid_cols   INT         NOT NULL DEFAULT 1,
    -- [{id, cell, coords, layout, link_group, ...}], the client owns the pane schema, the
    -- same contract the layout blob already has.
    panes       JSONB       NOT NULL DEFAULT '[]'::jsonb,
    -- Splitter fractions and link-group colors: {row_sizes: [..], col_sizes: [..], links: {..}}.
    settings    JSONB       NOT NULL DEFAULT '{}'::jsonb,
    position    INT         NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT histviz_workspaces_shape CHECK (grid_rows BETWEEN 1 AND 3 AND grid_cols BETWEEN 1 AND 4)
);

CREATE INDEX IF NOT EXISTS histviz_workspaces_position_idx ON histviz_workspaces (position, created_at);

-- Chart-owned instrument lists: the rail's second source next to the Watchlists module.
-- Deliberately separate from `watchlists_*`: a list built by clicking "add this instrument"
-- on a chart is a scratchpad, and promoting one into the Watchlists module is an explicit
-- action rather than a side effect.
CREATE TABLE IF NOT EXISTS histviz_lists (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT        NOT NULL,
    -- [{provider, asset_type, ticker, timeframe, connector_id, name}] in the user's order.
    items       JSONB       NOT NULL DEFAULT '[]'::jsonb,
    position    INT         NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS histviz_lists_position_idx ON histviz_lists (position, created_at);
