-- SPDX-License-Identifier: AGPL-3.0-or-later
-- A course's cover image (docs/specs/m1/06-files.md B19, Data): one PNG, JPEG or WebP course
-- file of the same course, or none.
--
-- The foreign key names the course's own id beside the cover's, so the database refuses a cover
-- that is a course file of another course, not only of another organization (the spec's two
-- columns). Its `NO ACTION` refuses deleting the course file a course uses as its cover, which
-- the repository answers first as 409 `file.in_use` (B11). The type is the repository's check:
-- `course_files` holds no type, its `files` row does.
ALTER TABLE courses
    ADD COLUMN cover_course_file_id uuid,
    ADD CONSTRAINT courses_cover FOREIGN KEY (org_id, id, cover_course_file_id)
        REFERENCES course_files (org_id, course_id, id);
CREATE INDEX courses_by_cover ON courses (org_id, cover_course_file_id)
    WHERE cover_course_file_id IS NOT NULL;
