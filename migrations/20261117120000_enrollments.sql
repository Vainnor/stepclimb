-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Enrollments (docs/specs/m1/02-courses-and-enrollments.md B15 to B22, Data; plan, Technical ->
-- data model `enrollments`). An enrollment puts a user in a course with a role; an `active` one
-- is the course-scope grant (Q1). The composite foreign keys keep the course and the user in the
-- enrollment's organization, and `enrollments_role_in_org` refuses another organization's custom
-- role, as `role_assignments_role_in_org` does for assignments. The foreign key to `roles`
-- refuses deleting a custom role that still has enrollments (spec 09 B10).
CREATE TABLE enrollments (
    id           uuid        PRIMARY KEY,
    org_id       uuid        NOT NULL,
    course_id    uuid        NOT NULL,
    user_id      uuid        NOT NULL,
    role_id      uuid        NOT NULL REFERENCES roles (id),
    status       text        NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'concluded')),
    source       text        NOT NULL DEFAULT 'local'
                             CHECK (source IN ('local', 'idp', 'sync', 'scim', 'connector')),
    external_id  text,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    UNIQUE (course_id, user_id, role_id)
);
CREATE INDEX enrollments_by_user ON enrollments (org_id, user_id) WHERE status = 'active';
CREATE INDEX enrollments_by_course ON enrollments (org_id, course_id, status);

CREATE FUNCTION enrollments_role_in_org() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM roles
        WHERE id = NEW.role_id AND org_id IS NOT NULL AND org_id <> NEW.org_id
    ) THEN
        RAISE EXCEPTION 'role % belongs to another organization', NEW.role_id
            USING ERRCODE = 'check_violation',
                  CONSTRAINT = 'enrollments_role_in_org',
                  TABLE = 'enrollments';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER enrollments_role_in_org
    AFTER INSERT OR UPDATE OF role_id, org_id ON enrollments
    FOR EACH ROW EXECUTE FUNCTION enrollments_role_in_org();

-- Student and Observer are course-scope roles, granted only by an enrollment (B22, owner
-- decision of 2026-11-16). M0 let them be assigned at the organization or an account, where
-- Student's `course.view` would show every published course in the scope; those assignments are
-- removed here, and the role assignment route refuses new ones (422
-- `role_assignment.course_role`). Course-scope assignments, which no route creates, are left.
DELETE FROM role_assignments ra
USING roles r
WHERE r.id = ra.role_id
  AND r.org_id IS NULL
  AND r.key IN ('student', 'observer')
  AND ra.scope_type IN ('organization', 'account');
