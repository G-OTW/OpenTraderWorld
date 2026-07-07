-- Trading Journal: per-category free-form description.
-- Shown (collapsible) at the top of the category breakdown so the user can jot
-- details about the category (its thesis, rules, scope, …).
ALTER TABLE journal_categories
    ADD COLUMN IF NOT EXISTS description TEXT;
