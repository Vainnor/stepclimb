-- SPDX-License-Identifier: AGPL-3.0-or-later
-- When a stored file last lost a reference (docs/specs/m1/06-files.md B21).
--
-- `files.cleanup` deletes a `files` row that nothing references once its last reference has been
-- gone for 24 hours. Deleting a course file stamps its file's `unreferenced_since` with the time
-- of the delete, so the newest stamp is when the last reference went. A row that has never lost
-- a reference keeps NULL, and cleanup reads it as unreferenced since `created_at`. A row that
-- gains a reference again keeps its stamp: cleanup only deletes a row with no reference left.
ALTER TABLE files ADD COLUMN unreferenced_since timestamptz;
