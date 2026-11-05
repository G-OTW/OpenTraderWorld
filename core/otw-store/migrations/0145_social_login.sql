-- Social sign-in: one external identity (Google, Microsoft, GitHub or any OpenID Connect
-- issuer) bound to the owner account, plus the recovery codes that get the owner back in
-- when that identity is locked, deleted or expired.

-- One row at most: the instance has one provider and one linked identity. The client
-- secret is sealed with OTW_SECRET_KEY like every other stored secret. `subject_iss` +
-- `subject` is the identity (the provider's stable user id), never the e-mail, which can
-- change hands. NULL until the owner completes the link from Settings.
CREATE TABLE IF NOT EXISTS social_login (
    id           BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    provider     TEXT        NOT NULL,
    issuer       TEXT        NOT NULL DEFAULT '',
    client_id    TEXT        NOT NULL,
    secret_nonce BYTEA,
    secret_ct    BYTEA,
    user_id      UUID REFERENCES users(id) ON DELETE CASCADE,
    subject_iss  TEXT,
    subject      TEXT,
    email        TEXT        NOT NULL DEFAULT '',
    linked_at    TIMESTAMPTZ,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Single-use recovery codes, stored as SHA-256 of the normalized code (80 random bits, so
-- a fast hash is enough). A used code keeps its row so the count left stays honest.
CREATE TABLE IF NOT EXISTS recovery_codes (
    user_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash  TEXT        NOT NULL,
    used_at    TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, code_hash)
);

-- Username + password sign-in. Only ever turned off while a social identity is linked;
-- unlinking it, a recovery-code sign-in and the host CLI reset all turn it back on.
ALTER TABLE users ADD COLUMN IF NOT EXISTS password_login BOOLEAN NOT NULL DEFAULT TRUE;
