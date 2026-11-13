-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Accounts: the tree inside an organization (docs/specs/m0/02-organizations-and-accounts.md B16
-- to B22). `path` holds the ids of the root, every ancestor and the account itself, each as 32
-- lowercase hex digits; the repository computes it under a per-organization advisory lock, and
-- keeps it equal to the parent's path plus the account's id, which no constraint can express.
-- ltree is a trusted extension since PostgreSQL 13, so the database owner can create it.
CREATE EXTENSION IF NOT EXISTS ltree;

CREATE TABLE accounts (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    parent_id   uuid,
    name        text        NOT NULL CHECK (btrim(name) <> ''),
    path        ltree       NOT NULL,
    settings    jsonb       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(settings) = 'object'),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    -- Lets later tables point at an account with (org_id, account_id), so they cannot reference
    -- another organization's account.
    UNIQUE (org_id, id),
    -- A parent is in the same organization as its child; the default RESTRICT refuses deleting
    -- an account that still has children.
    FOREIGN KEY (org_id, parent_id) REFERENCES accounts (org_id, id),
    CHECK ((parent_id IS NULL) = (nlevel(path) = 1))
);
CREATE UNIQUE INDEX accounts_one_root ON accounts (org_id) WHERE parent_id IS NULL;
CREATE INDEX accounts_parent ON accounts (org_id, parent_id, name, id);
CREATE INDEX accounts_path ON accounts USING gist (path);
