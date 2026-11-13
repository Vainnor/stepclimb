-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Users: the people of one organization (docs/specs/m0/03-users-and-identities.md B1 to B8).
-- `tz` has no default: the default is the organization's setting (Q4), which the caller
-- resolves before inserting. `email` is unique per organization, compared under lower() (B12, Q1).
CREATE TABLE users (
    id            uuid        PRIMARY KEY,
    org_id        uuid        NOT NULL REFERENCES organizations (id),
    external_id   text,
    source        text        NOT NULL CHECK (source IN ('local', 'idp', 'sync', 'scim', 'connector')),
    manager_id    uuid,
    display_name  text        NOT NULL CHECK (btrim(display_name) <> ''),
    given_name    text        NOT NULL DEFAULT '',
    family_name   text        NOT NULL DEFAULT '',
    email         text,
    status        text        NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'suspended')),
    profile       jsonb       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(profile) = 'object'),
    locale        text        NOT NULL DEFAULT 'en',
    tz            text        NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    -- A manager is in the same organization: the composite key cannot point across tenants.
    FOREIGN KEY (org_id, manager_id) REFERENCES users (org_id, id),
    CHECK (manager_id IS NULL OR manager_id <> id)
);
CREATE UNIQUE INDEX users_external_id ON users (org_id, external_id) WHERE external_id IS NOT NULL;
CREATE INDEX users_by_name ON users (org_id, display_name, id);
CREATE UNIQUE INDEX users_email ON users (org_id, lower(email)) WHERE email IS NOT NULL;
