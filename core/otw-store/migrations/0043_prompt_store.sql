-- PromptStore module.
--
-- A library of saved prompts shown as a grid of vignettes (name, tags, last-saved).
-- Each prompt keeps an immutable version history so the user can roll back to an
-- earlier body. The `prompts` row always mirrors the latest version's content; a
-- rollback appends a new version (copying an old one) rather than deleting history.
-- Single-user: no owner scoping.

CREATE TABLE IF NOT EXISTS prompt_store_prompts (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL DEFAULT '',
    body       TEXT NOT NULL DEFAULT '',
    -- Free-form tags as a text array; empty array means untagged.
    tags       TEXT[] NOT NULL DEFAULT '{}',
    -- Quick classification: 1 = thumbs up, -1 = thumbs down, 0 = unrated.
    vote       SMALLINT NOT NULL DEFAULT 0,
    -- Monotonic version counter; matches the latest prompt_store_versions.version.
    version    INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Bumped on every content edit / rollback; drives the "last save" label.
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS prompt_store_versions (
    id         UUID PRIMARY KEY,
    prompt_id  UUID NOT NULL REFERENCES prompt_store_prompts(id) ON DELETE CASCADE,
    -- 1-based, unique per prompt; ascending in save order.
    version    INTEGER NOT NULL,
    name       TEXT NOT NULL DEFAULT '',
    body       TEXT NOT NULL DEFAULT '',
    tags       TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (prompt_id, version)
);

CREATE INDEX IF NOT EXISTS idx_prompt_store_vote ON prompt_store_prompts(vote);
CREATE INDEX IF NOT EXISTS idx_prompt_store_versions_prompt
    ON prompt_store_versions(prompt_id, version DESC);
