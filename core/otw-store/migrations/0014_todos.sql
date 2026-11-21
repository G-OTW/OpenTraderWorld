-- ToDo module.
--
-- A simple task list: name, optional due date, free-form details, and a done flag.
-- Single-user: no owner scoping.

CREATE TABLE IF NOT EXISTS todos (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL DEFAULT '',
    due_date   DATE,
    details    TEXT NOT NULL DEFAULT '',
    done       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_todos_done ON todos(done);
