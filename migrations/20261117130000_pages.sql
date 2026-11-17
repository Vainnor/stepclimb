-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Pages, their versions and the course files a version's body names
-- (docs/specs/m1/07-pages-and-the-editor.md B1, B2, B10, Data; plan, Technical -> data model
-- `pages`, `page_versions`).
--
-- A page belongs to one course and always has a current version: the page row and its first
-- version are written in one transaction, and the deferred foreign key on `current_version_id`
-- is checked at commit. That key names `(id, current_version_id)`, so the current version is
-- always one of the page's own; `current_version_id` is NOT NULL for the same reason (B1).
--
-- A version is immutable while its page exists (B2): `page_versions_immutable` refuses an
-- `UPDATE` of its body, number, page or author. Deleting the page deletes its versions.
-- `number` is 1, 2, ... per page; the repository takes the next one under the page's row lock.
--
-- `page_version_files` lists the course files a version's body names as images, written on
-- save from the parsed body. Its `NO ACTION` foreign key to `course_files` refuses deleting a
-- course file any version uses, which `course_files::delete` checks first so the route answers
-- 409 `file.in_use` (spec 06 B11). Every foreign key keeps a row in its organization.
CREATE TABLE pages (
    id                  uuid        PRIMARY KEY,
    org_id              uuid        NOT NULL,
    course_id           uuid        NOT NULL,
    slug                text        NOT NULL CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND char_length(slug) <= 100),
    title               text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    current_version_id  uuid        NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (course_id, slug),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id)
);
CREATE INDEX pages_by_course ON pages (org_id, course_id, slug);

CREATE TABLE page_versions (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL,
    page_id     uuid        NOT NULL,
    number      integer     NOT NULL CHECK (number >= 1),
    body        jsonb       NOT NULL CHECK (jsonb_typeof(body) = 'object'),
    author_id   uuid        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (page_id, id),
    UNIQUE (page_id, number),
    FOREIGN KEY (org_id, page_id) REFERENCES pages (org_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, author_id) REFERENCES users (org_id, id)
);

ALTER TABLE pages ADD CONSTRAINT pages_current_version
    FOREIGN KEY (id, current_version_id) REFERENCES page_versions (page_id, id)
    DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION page_versions_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.body IS DISTINCT FROM OLD.body
       OR NEW.number IS DISTINCT FROM OLD.number
       OR NEW.page_id IS DISTINCT FROM OLD.page_id
       OR NEW.author_id IS DISTINCT FROM OLD.author_id THEN
        RAISE EXCEPTION 'a page version cannot be changed'
            USING ERRCODE = 'restrict_violation',
                  CONSTRAINT = 'page_versions_immutable',
                  TABLE = 'page_versions';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER page_versions_immutable
    BEFORE UPDATE ON page_versions
    FOR EACH ROW EXECUTE FUNCTION page_versions_immutable();

CREATE TABLE page_version_files (
    org_id          uuid        NOT NULL,
    version_id      uuid        NOT NULL,
    course_file_id  uuid        NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (version_id, course_file_id),
    FOREIGN KEY (org_id, version_id) REFERENCES page_versions (org_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, course_file_id) REFERENCES course_files (org_id, id)
);
CREATE INDEX page_version_files_by_file ON page_version_files (course_file_id);
