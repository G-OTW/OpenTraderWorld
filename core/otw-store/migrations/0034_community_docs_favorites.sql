-- Community Docs favorites.
--
-- A user pins docs to a persistent left-pane favorites list. Favorites are keyed by the
-- doc `slug` (the stable source id) rather than the row id so a favorite survives a
-- re-sync that replaces the underlying `community_docs` row. On delete of the doc the
-- favorite is cleaned up via FK. A future "refresh from source" must never touch this
-- table. Single-user: no owner scoping.

CREATE TABLE IF NOT EXISTS community_docs_favorites (
    slug         TEXT PRIMARY KEY REFERENCES community_docs(slug) ON DELETE CASCADE,
    favorited_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
