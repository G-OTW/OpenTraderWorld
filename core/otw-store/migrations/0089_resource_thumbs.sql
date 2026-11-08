-- Resources — a thumbnail per bookmark, for the gallery display.
--
-- One text column rather than a file reference: the image can be an uploaded file served
-- back as `/api/files/{id}` (that is also where an auto-fetched social preview lands) or
-- any URL the user pastes. Empty = no thumbnail, and the UI draws its own placeholder.
ALTER TABLE resources ADD COLUMN IF NOT EXISTS thumb_url TEXT NOT NULL DEFAULT '';
