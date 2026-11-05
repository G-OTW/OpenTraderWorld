-- Force a password change on first login for admins created non-interactively (the
-- headless install bootstraps the admin from OTW_ADMIN_PASSWORD, an auto-generated value
-- that lands in .env — the operator should replace it with one of their own choosing).
-- Cleared the first time the account's password is changed. Default FALSE so accounts
-- created through the browser wizard (where the operator chose the password) are unaffected.
ALTER TABLE users
    ADD COLUMN must_change_password BOOLEAN NOT NULL DEFAULT FALSE;
