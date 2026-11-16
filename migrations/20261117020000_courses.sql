-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Courses (docs/specs/m1/02-courses-and-enrollments.md B1 to B11, Data; plan, Technical ->
-- data model `courses`). A course belongs to one account of its organization; the composite
-- foreign key keeps the account in the course's organization, and its default `NO ACTION`
-- refuses deleting an account that holds a course (B8), which `accounts::delete` checks first so
-- the route answers 409 `account.has_courses`.
--
-- `template` is not in the plan's data model: it is Q5's flag. A template is never published.
-- `settings` is read and written only through `stepclimb_core::courses::CourseSettings`.
-- `code` is unique per organization, compared ASCII-case-insensitively (Q2).
CREATE TABLE courses (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    account_id  uuid        NOT NULL,
    code        text        NOT NULL CHECK (char_length(code) BETWEEN 1 AND 50),
    title       text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    status      text        NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published', 'archived')),
    template    boolean     NOT NULL DEFAULT false,
    start_at    timestamptz,
    end_at      timestamptz,
    settings    jsonb       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(settings) = 'object'),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    FOREIGN KEY (org_id, account_id) REFERENCES accounts (org_id, id),
    CHECK (end_at IS NULL OR start_at IS NULL OR end_at >= start_at),
    CHECK (NOT (template AND status = 'published'))
);
CREATE UNIQUE INDEX courses_code ON courses (org_id, lower(code COLLATE "C"));
CREATE INDEX courses_by_account ON courses (org_id, account_id, title, id);
