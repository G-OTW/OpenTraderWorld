-- Mindset: one free-form note per day, written in the same rich-text editor as the Editor
-- module. `content` is the ProseMirror JSON document; a day with no note has no row.
CREATE TABLE mindset_day_notes (
    day DATE PRIMARY KEY,
    content JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
