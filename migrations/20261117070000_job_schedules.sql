-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Periodic jobs (docs/specs/m1/01-job-queue-and-outbox.md B17, Data), and the index
-- `jobs.prune` reads dead jobs through.
--
-- `job_schedules`: one row per registered
-- periodic kind, named after the kind. The scheduler enqueues an instance job for each row whose
-- `next_run_at` has come and moves it on by `interval_s`. Instance-wide like `plans`: no
-- organization's data, so no `org_id`.
CREATE TABLE job_schedules (
    name          text        PRIMARY KEY,
    interval_s    integer     NOT NULL CHECK (interval_s >= 10),
    next_run_at   timestamptz NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

-- `jobs.prune` deletes each organization's dead jobs, then the instance's, in batches by
-- `org_id` and `dead_at` (B8, B18). Only dead rows are indexed.
CREATE INDEX jobs_dead ON jobs (org_id, dead_at) WHERE dead_at IS NOT NULL;
