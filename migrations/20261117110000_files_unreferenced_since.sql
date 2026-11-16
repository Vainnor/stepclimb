-- SPDX-License-Identifier: AGPL-3.0-or-later
-- When a stored file last lost a reference, and no format character in a course file's path
-- (docs/specs/m1/06-files.md B9, B21).
--
-- `files.cleanup` deletes a `files` row that nothing references once its last reference has been
-- gone for 24 hours. Deleting a course file stamps its file's `unreferenced_since` with the time
-- of the delete, so the newest stamp is when the last reference went. A row that has never lost
-- a reference keeps NULL, and cleanup reads it as unreferenced since `created_at`. A row that
-- gains a reference again keeps its stamp: cleanup only deletes a row with no reference left.
ALTER TABLE files ADD COLUMN unreferenced_since timestamptz;

-- B9: no Unicode format character (general category Cf, Unicode 16.0: bidirectional overrides
-- and isolates, zero-width characters, the byte order mark, tags) in a folder or a name, so a
-- name cannot show as another. The ranges are `stepclimb_core::users::FORMAT_CHARS`, as the
-- `groups` key check spells them; `course_files::Folder` and `course_files::Name` refuse the same.
ALTER TABLE course_files ADD CONSTRAINT course_files_no_format_characters CHECK (
    folder || '/' || name !~ ('[­؀-؅؜۝܏࢐-࢑࣢᠎'
                              '​-‏‪-‮⁠-⁤⁦-⁯﻿'
                              '￹-￻\U000110bd\U000110cd\U00013430-\U0001343f'
                              '\U0001bca0-\U0001bca3\U0001d173-\U0001d17a\U000e0001'
                              '\U000e0020-\U000e007f]')
);
