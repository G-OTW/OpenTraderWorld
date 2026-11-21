-- Body and attachment size caps are constants in the ingest code, not user settings:
-- they exist to keep one sender from filling the database, which is not a preference.
ALTER TABLE mailbox_settings
    DROP COLUMN IF EXISTS max_body_kb,
    DROP COLUMN IF EXISTS max_attachment_kb;
