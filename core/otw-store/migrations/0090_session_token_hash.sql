-- Session tokens at rest: store SHA-256, never the token itself.
--
-- `sessions.token` held the raw bearer value, so any read of the database — an unencrypted
-- .sql backup, a leaked replica, a future read-only injection — yielded directly replayable
-- sessions valid for up to a week. MCP and webhook tokens were already hashed
-- (otw-store/src/mcp.rs `hash_token`); sessions were the outlier.
--
-- Existing rows cannot be migrated (the hash is one-way and the plaintext is only in the
-- user's cookie), so they are dropped: everyone signs in once more after the update. That
-- is the correct trade for a single-user app.

DELETE FROM sessions;

ALTER TABLE sessions RENAME COLUMN token TO token_hash;
