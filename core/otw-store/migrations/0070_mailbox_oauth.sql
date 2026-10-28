-- Mailbox: OAuth 2.0 accounts (Microsoft / Outlook.com / Microsoft 365).
--
-- Microsoft removed password authentication for IMAP, so those mailboxes authenticate with
-- XOAUTH2. The long-lived credential is the *refresh token*, which lives in the vault like
-- every other secret; the access token is short-lived and never persisted.

ALTER TABLE mailbox_accounts
    -- OAuth accounts have no password to store.
    ALTER COLUMN vault_item_id DROP NOT NULL;

ALTER TABLE mailbox_accounts
    ADD COLUMN IF NOT EXISTS auth_kind          TEXT NOT NULL DEFAULT 'password',
    ADD COLUMN IF NOT EXISTS oauth_provider     TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS oauth_client_id    TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS oauth_tenant       TEXT NOT NULL DEFAULT 'common',
    ADD COLUMN IF NOT EXISTS refresh_vault_item UUID REFERENCES vault_items(id),
    -- Last time a refresh token was successfully exchanged. Microsoft's refresh tokens
    -- expire after 90 days of *inactivity*, so this is the clock that matters.
    ADD COLUMN IF NOT EXISTS token_refreshed_at TIMESTAMPTZ,
    -- The credential was revoked (password change, MFA reset, admin action, policy) and
    -- only the user can fix it. Polling stops until they reconnect.
    ADD COLUMN IF NOT EXISTS needs_reauth       BOOLEAN NOT NULL DEFAULT FALSE;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'mailbox_accounts_auth_kind_chk') THEN
        ALTER TABLE mailbox_accounts
            ADD CONSTRAINT mailbox_accounts_auth_kind_chk
            CHECK (auth_kind IN ('password', 'oauth'));
    END IF;
    -- A password account without its vault item could never connect; an OAuth account
    -- legitimately has no credential until the first device-code sign-in completes.
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'mailbox_accounts_password_cred_chk') THEN
        ALTER TABLE mailbox_accounts
            ADD CONSTRAINT mailbox_accounts_password_cred_chk
            CHECK (auth_kind <> 'password' OR vault_item_id IS NOT NULL);
    END IF;
END
$$;
