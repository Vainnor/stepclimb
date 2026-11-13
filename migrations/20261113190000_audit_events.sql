-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Audit entries for permission and account changes (docs/specs/m0/09-permissions-and-roles.md
-- Q5, docs/specs/m0/08-sessions-and-csrf.md Q14; plan, Technical -> data model). The repository
-- function that makes a change writes its entry in the same transaction
-- (`stepclimb_core::audit`), so an entry exists exactly when its change committed.
--
-- The plan's single `actor` column is `actor_type` and `actor_id` here: the instance itself (the
-- `stepclimb admin` CLI) has no id, and spec 08's service accounts will be a third type. A user
-- actor always has the client address the server derived for the request; the instance never
-- has one. `actor_id` and `target_id` have no foreign key: an entry outlives the user, role or
-- account it names. `at` is the server clock's reading of the change; `created_at` and
-- `updated_at` are spec 02 B5's, and an entry is never updated.
CREATE TABLE audit_events (
    id           uuid        PRIMARY KEY,
    org_id       uuid        NOT NULL REFERENCES organizations (id),
    actor_type   text        NOT NULL CHECK (actor_type IN ('user', 'instance')),
    actor_id     uuid,
    action       text        NOT NULL,
    target_type  text        NOT NULL,
    target_id    uuid        NOT NULL,
    data         jsonb       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(data) = 'object'),
    at           timestamptz NOT NULL,
    ip           inet,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    CHECK ((actor_type = 'user') = (actor_id IS NOT NULL AND ip IS NOT NULL)),
    CHECK (actor_type <> 'instance' OR (actor_id IS NULL AND ip IS NULL))
);
CREATE INDEX audit_events_by_time ON audit_events (org_id, at, id);
CREATE INDEX audit_events_by_target ON audit_events (org_id, target_type, target_id);
