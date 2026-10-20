-- Mindset: pre-mortem / post-mortem check-ins built from customizable prompts.

CREATE TABLE mindset_prompts (
    id UUID PRIMARY KEY,
    -- Which check-in the prompt belongs to: pre (before the session) | post (after).
    phase TEXT NOT NULL,
    -- Control type: scale (1–5) | choice (pick one) | tags (pick many) | text (free form).
    kind TEXT NOT NULL,
    label TEXT NOT NULL,
    -- Kind-specific config: {"low":"…","high":"…"} for scale; {"options":[…]} for choice/tags.
    config JSONB NOT NULL DEFAULT '{}',
    position DOUBLE PRECISION NOT NULL DEFAULT 0,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One check-in per (date, phase); answers keyed by prompt id.
CREATE TABLE mindset_entries (
    id UUID PRIMARY KEY,
    entry_date DATE NOT NULL,
    phase TEXT NOT NULL,
    answers JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (entry_date, phase)
);
