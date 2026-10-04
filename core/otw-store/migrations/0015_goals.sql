-- Goals module.
--
-- Each goal carries a name, optional deadline, free-form details, and a JSONB array of
-- KPIs. Progress is derived as reached-points / total-points. Single-user.
--
-- `kpis` is an array of objects:
--   [{ "name": str, "target": num, "current": num, "reached": bool, "points": num }, …]
-- Stored as JSONB (mirrors the wealth `fields` approach); validated in the API layer.

CREATE TABLE IF NOT EXISTS goals (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL DEFAULT '',
    deadline   DATE,
    details    TEXT NOT NULL DEFAULT '',
    kpis       JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
