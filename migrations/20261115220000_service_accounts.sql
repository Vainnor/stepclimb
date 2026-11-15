-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Service accounts and their API keys (docs/specs/m0/08-sessions-and-csrf.md B24 to B29, Data,
-- Q9). One row is one API key: rotating a key is a new service account. Every service account
-- has an owner user of its organization (Q9 (a)); the key's rights on each request are its
-- `scopes`, catalog permission keys, intersected with the owner's current permissions.
-- `secret_hash` is the SHA-256 of the key's text (Q2), never the key itself: the length check
-- refuses the 53-character key. `kind` has one value in M0; `name` and `last_used_at` are Q9's
-- additions to the plan's columns, and there is no expiry in M0.
CREATE TABLE service_accounts (
    id             uuid        PRIMARY KEY,
    org_id         uuid        NOT NULL REFERENCES organizations (id),
    kind           text        NOT NULL CHECK (kind IN ('api_key')),
    name           text        NOT NULL CHECK (name <> ''),
    owner_user_id  uuid        NOT NULL,
    secret_hash    bytea       NOT NULL UNIQUE CHECK (octet_length(secret_hash) = 32),
    scopes         text[]      NOT NULL,
    last_used_at   timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, owner_user_id) REFERENCES users (org_id, id) ON DELETE CASCADE
);
CREATE INDEX service_accounts_by_owner ON service_accounts (org_id, owner_user_id);

-- An API key is the third kind of audit actor (docs/specs/m0/09-permissions-and-roles.md B23):
-- `actor_id` is the service account's id, and like a user it has the client address the server
-- derived for the request.
ALTER TABLE audit_events
    DROP CONSTRAINT audit_events_actor_type_check,
    DROP CONSTRAINT audit_events_user_actor,
    ADD CONSTRAINT audit_events_actor_type_check
        CHECK (actor_type IN ('user', 'instance', 'api_key')),
    ADD CONSTRAINT audit_events_user_actor
        CHECK (actor_type = 'instance' OR (actor_id IS NOT NULL AND ip IS NOT NULL));
