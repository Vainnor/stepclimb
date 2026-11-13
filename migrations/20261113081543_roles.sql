-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Roles and role assignments (docs/specs/m0/09-permissions-and-roles.md B4, B6, B11, B15, Data).
-- Built-in roles are instance-wide rows (`org_id` null) that the server upserts from code at
-- startup; custom roles belong to one organization. The server reads a built-in role's
-- permissions from code, never from its row (B6): the row's `permissions` is for SQL readers.
CREATE TABLE roles (
    id           uuid        PRIMARY KEY,
    org_id       uuid        REFERENCES organizations (id),
    key          text        NOT NULL,
    name         text        NOT NULL CHECK (btrim(name) <> ''),
    builtin      boolean     NOT NULL,
    permissions  text[]      NOT NULL DEFAULT '{}',
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    CHECK (builtin = (org_id IS NULL))
);
CREATE UNIQUE INDEX roles_builtin_key ON roles (key) WHERE org_id IS NULL;
CREATE UNIQUE INDEX roles_org_key ON roles (org_id, key) WHERE org_id IS NOT NULL;

-- A role never changes organization, key or kind: the assignment trigger below relies on a
-- role's `org_id` staying put, and an assignment keeps meaning the role it was made for.
CREATE FUNCTION roles_identity_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.org_id, NEW.key, NEW.builtin) IS DISTINCT FROM (OLD.org_id, OLD.key, OLD.builtin) THEN
        RAISE EXCEPTION 'the organization, key and builtin flag of role % cannot change', OLD.id
            USING ERRCODE = 'check_violation',
                  CONSTRAINT = 'roles_identity_immutable',
                  TABLE = 'roles';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER roles_identity_immutable
    BEFORE UPDATE ON roles
    FOR EACH ROW EXECUTE FUNCTION roles_identity_immutable();

-- `scope_id` has no foreign key: it points at the organization, an account or (from M1) a
-- course, by `scope_type`. The repository checks that an account scope exists, and deleting an
-- account deletes the assignments scoped to it (B15). The default action of the role foreign
-- key, NO ACTION, refuses deleting a role that still has assignments (B10).
CREATE TABLE role_assignments (
    id           uuid        PRIMARY KEY,
    org_id       uuid        NOT NULL,
    user_id      uuid        NOT NULL,
    role_id      uuid        NOT NULL REFERENCES roles (id),
    scope_type   text        NOT NULL CHECK (scope_type IN ('organization', 'account', 'course')),
    scope_id     uuid        NOT NULL,
    source       text        NOT NULL DEFAULT 'local'
                             CHECK (source IN ('local', 'idp', 'sync', 'scim', 'connector')),
    external_id  text,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    UNIQUE (user_id, role_id, scope_type, scope_id),
    CHECK (scope_type <> 'organization' OR scope_id = org_id)
);
CREATE INDEX role_assignments_by_user ON role_assignments (org_id, user_id);
CREATE INDEX role_assignments_by_scope ON role_assignments (org_id, scope_type, scope_id);

-- A custom role can be assigned only in its own organization; a built-in role in any. A foreign
-- key cannot say this because a built-in role's `org_id` is null. A role's `org_id` never
-- changes (`roles_identity_immutable`), so checking the assignment row on insert and on a
-- change of its role or organization is enough. The error names the trigger as its constraint,
-- so the repository can map it.
CREATE FUNCTION role_assignments_role_in_org() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM roles
        WHERE id = NEW.role_id AND org_id IS NOT NULL AND org_id <> NEW.org_id
    ) THEN
        RAISE EXCEPTION 'role % belongs to another organization', NEW.role_id
            USING ERRCODE = 'check_violation',
                  CONSTRAINT = 'role_assignments_role_in_org',
                  TABLE = 'role_assignments';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER role_assignments_role_in_org
    AFTER INSERT OR UPDATE OF role_id, org_id ON role_assignments
    FOR EACH ROW EXECUTE FUNCTION role_assignments_role_in_org();
