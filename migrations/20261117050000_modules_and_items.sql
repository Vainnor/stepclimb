-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Modules and their items (docs/specs/m1/03-modules-and-items.md B1 to B12, Data; plan,
-- Technical -> data model `modules`, `module_items`). A course is an ordered list of modules,
-- and a module an ordered list of items.
--
-- `position` is 0-based and dense within the course (modules) or the module (items). Every
-- structural write locks the course row first (B12); the deferrable unique constraints back that
-- lock and let one statement shift a run of positions.
--
-- `module_items.course_id` repeats the module's course. The three-column foreign key keeps it
-- equal to the module's, so it cannot drift and an item moved between modules stays in its
-- course (B11). `ref_id` has no foreign key: it points at a different table per kind, and the
-- repository checks it (B6). `published` on both tables and `url` are not in the plan's data
-- model: they are Q1's switch and B7's link.
--
-- `stepclimb_core::items::url_is_valid` checks an external link's URL in full (B7); the
-- `url` check is a backstop that keeps any writer to absolute http and https URLs.
--
-- `quiz` and `activity` are in the kind list so M2 and M3 add no migration to it; no M1 code
-- creates them.
CREATE TABLE modules (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL,
    course_id   uuid        NOT NULL,
    position    integer     NOT NULL CHECK (position >= 0),
    title       text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    published   boolean     NOT NULL DEFAULT false,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (org_id, course_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    CONSTRAINT modules_position UNIQUE (course_id, position) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE module_items (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL,
    course_id   uuid        NOT NULL,
    module_id   uuid        NOT NULL,
    position    integer     NOT NULL CHECK (position >= 0),
    kind        text        NOT NULL CHECK (kind IN ('page', 'video', 'file', 'quiz', 'activity', 'external_link', 'header')),
    ref_id      uuid,
    url         text        CHECK (char_length(url) <= 2048),
    title       text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    published   boolean     NOT NULL DEFAULT false,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    FOREIGN KEY (org_id, course_id, module_id) REFERENCES modules (org_id, course_id, id),
    CONSTRAINT module_items_position UNIQUE (module_id, position) DEFERRABLE INITIALLY DEFERRED,
    CHECK ((kind = 'external_link') = (url IS NOT NULL)),
    CHECK ((kind IN ('external_link', 'header')) = (ref_id IS NULL)),
    CHECK (url IS NULL OR url ~* '^https?://')
);
CREATE INDEX module_items_ref ON module_items (org_id, kind, ref_id) WHERE ref_id IS NOT NULL;
