-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Periodic jobs (docs/specs/m1/01-job-queue-and-outbox.md B17, Data): one row per registered
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
