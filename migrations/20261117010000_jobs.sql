-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The job queue (docs/specs/m1/01-job-queue-and-outbox.md B1 to B11, Data). One row per job that
-- is waiting, running or dead; a finished job's row is deleted. Workers claim due rows with
-- `FOR UPDATE SKIP LOCKED` and hold them under the lease in `locked_until`. `org_id` is null for
-- an instance job, which is not organization data (as `roles.org_id`).
CREATE TABLE jobs (
    id            uuid        PRIMARY KEY,
    org_id        uuid        REFERENCES organizations (id),
    queue         text        NOT NULL CHECK (queue ~ '^[a-z][a-z0-9_]{0,31}$'),
    kind          text        NOT NULL CHECK (char_length(kind) BETWEEN 3 AND 100),
    payload       jsonb       NOT NULL,
    dedupe_key    text        CHECK (char_length(dedupe_key) BETWEEN 1 AND 200),
    run_at        timestamptz NOT NULL,
    attempts      integer     NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts  integer     NOT NULL CHECK (max_attempts >= 1),
    locked_until  timestamptz,
    last_error    text        CHECK (octet_length(last_error) <= 2048),
    dead_at       timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);
-- The claim's scan: due rows of a queue in `run_at, id` order (B4).
CREATE INDEX jobs_due ON jobs (queue, run_at, id) WHERE dead_at IS NULL;
-- At most one waiting (never claimed, or released) job per kind, organization and key (B3).
-- `NULLS NOT DISTINCT` so instance jobs (null `org_id`) dedupe among themselves too.
CREATE UNIQUE INDEX jobs_dedupe_waiting ON jobs (kind, org_id, dedupe_key) NULLS NOT DISTINCT
    WHERE dedupe_key IS NOT NULL AND locked_until IS NULL AND dead_at IS NULL;
CREATE INDEX jobs_by_org ON jobs (org_id) WHERE org_id IS NOT NULL;
