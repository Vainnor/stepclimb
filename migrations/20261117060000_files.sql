-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Stored files and the course files that name them (docs/specs/m1/06-files.md B2, B7, B9, B22,
-- Data; plan, Technical -> data model `files`, `course_files`).
--
-- A `files` row is one distinct file of one organization: `UNIQUE (org_id, sha256)` stores the
-- same bytes once per organization (Q9), and its object lives at `storage_key`,
-- `orgs/<org_id>/files/<sha256 hex>` (B2). `size` is what the `storage` limit sums (B22).
--
-- A `course_files` row places a file in a folder of a course under a name. `id` is not in the
-- plan's data model: items and routes name a course file. The checks hold B9's rules: `folder`
-- is `/` or 1 to 8 segments and `name` one segment of up to 255 characters; a segment is 1 to 100
-- characters, not `.` or `..`, with no `/`, `\` or control character (C0, DEL, C1; PostgreSQL
-- text holds no NUL). NFC normalization is the repository's (I2). Both composite foreign keys
-- keep the course and the file in the row's organization, and `file_id`'s default `NO ACTION`
-- stops a referenced `files` row from being deleted (B21).
CREATE TABLE files (
    id           uuid        PRIMARY KEY,
    org_id       uuid        NOT NULL REFERENCES organizations (id),
    sha256       bytea       NOT NULL CHECK (octet_length(sha256) = 32),
    size         bigint      NOT NULL CHECK (size >= 0),
    mime         text        NOT NULL CHECK (char_length(mime) BETWEEN 3 AND 100),
    storage_key  text        NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, sha256),
    UNIQUE (org_id, id)
);

CREATE TABLE course_files (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL,
    course_id   uuid        NOT NULL,
    file_id     uuid        NOT NULL,
    folder      text        NOT NULL CHECK (
        folder = '/'
        OR (folder ~ '^(/[^/\\\u0001-\u001f\u007f-\u009f]{1,100}){1,8}$'
            AND folder !~ '/\.\.?(/|$)')
    ),
    name        text        NOT NULL CHECK (
        name ~ '^[^/\\\u0001-\u001f\u007f-\u009f]{1,255}$' AND name NOT IN ('.', '..')
    ),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (course_id, folder, name),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, file_id) REFERENCES files (org_id, id)
);
CREATE INDEX course_files_by_folder ON course_files (org_id, course_id, folder, name);
CREATE INDEX course_files_by_file ON course_files (file_id);
