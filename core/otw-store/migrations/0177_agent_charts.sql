-- Charts the agent draws in a conversation. The server reads the source series in-process,
-- reduces it (LTTB for lines, bins for histograms, top-N for donuts) and stores the reduced
-- result here; the model only sees a short summary and the chart id. The conversation
-- renders the row from GET /api/agent/charts/{id}, so a reload shows the same picture.

CREATE TABLE agent_charts (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES agent_conversations(id) ON DELETE CASCADE,
    -- The call as the model made it (type, title, path, x, y…).
    spec            JSONB NOT NULL,
    -- The reduced, render-ready series.
    data            JSONB NOT NULL,
    points_raw      INTEGER NOT NULL DEFAULT 0,
    points_shown    INTEGER NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX agent_charts_conv ON agent_charts (conversation_id);
