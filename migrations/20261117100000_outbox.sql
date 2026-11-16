-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The transactional outbox (docs/specs/m1/01-job-queue-and-outbox.md B19 to B23, Data). One row
-- per event, written in the transaction of the change it reports. `payload` is the event
-- envelope `{ "type", "timestamp", "data" }`; the row's id is the event id. `version` is the
-- schema version of the topic's `data` the emitting release wrote. The dispatcher sets
-- `dispatched_at` once it has enqueued a job for each subscriber of the topic; `jobs.prune`
-- deletes dispatched rows 30 days later and never an undispatched one.
CREATE TABLE outbox (
    id             uuid        PRIMARY KEY,
    org_id         uuid        NOT NULL REFERENCES organizations (id),
    topic          text        NOT NULL CHECK (topic ~ '^[a-z][a-z_]*(\.[a-z][a-z_]*)+$'),
    version        integer     NOT NULL CHECK (version >= 1),
    payload        jsonb       NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
    dispatched_at  timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now()
);
-- The dispatcher's claim: undispatched rows in `id` order, with the topic and version it filters
-- on (B21); also the count of undispatched rows of topics a release does not know.
CREATE INDEX outbox_undispatched ON outbox (id) INCLUDE (topic, version)
    WHERE dispatched_at IS NULL;
-- `jobs.prune` deletes each organization's dispatched rows by `org_id` and `dispatched_at`
-- (B23), as `jobs_dead` serves its dead jobs.
CREATE INDEX outbox_dispatched ON outbox (org_id, dispatched_at) WHERE dispatched_at IS NOT NULL;
CREATE INDEX outbox_by_org ON outbox (org_id, id);
