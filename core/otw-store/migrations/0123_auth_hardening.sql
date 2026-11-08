-- Auth hardening: a second factor, session provenance, and the known-source list that
-- makes a "new sign-in" notification possible.

-- TOTP shared secret, sealed with OTW_SECRET_KEY like every other stored secret
-- (XChaCha20-Poly1305: nonce + ciphertext). NULL until the user enrols. `totp_enabled`
-- is separate from "a secret exists" because enrolment is two steps: the secret is stored
-- when the QR code is shown, and only a correct code from the authenticator flips the flag.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS totp_nonce   BYTEA,
    ADD COLUMN IF NOT EXISTS totp_secret  BYTEA,
    ADD COLUMN IF NOT EXISTS totp_enabled BOOLEAN NOT NULL DEFAULT FALSE;

-- Session provenance, for the active-sessions list in Settings. Defaults keep every row
-- written before this migration valid; an empty string reads as "unknown" in the UI.
ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS ip           TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS user_agent   TEXT NOT NULL DEFAULT '';

-- Sources this account has signed in from before. A login from an address absent here
-- raises a notification; the row is then added, so the alert fires once per source and
-- not on every sign-in. Kept out of `sessions` on purpose: that table is pruned on expiry
-- and the point of this one is to remember past sources.
CREATE TABLE IF NOT EXISTS login_ips (
    user_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    ip         TEXT        NOT NULL,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, ip)
);
