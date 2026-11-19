-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Completion history and validity (docs/specs/m1/14-recurrence-and-expiring-completions.md B1
-- to B3, Data; plan, Technical -> data model `completions`).
--
-- One `completions` row per course or module completion per cycle, written in the transaction
-- where spec 05 sets the module or course `completed_at`. `cycle` counts the user's completions
-- of that target, from 1. `expires_at` is `completed_at` plus the target's
-- `completion_valid_months` in the organization's time zone when it has one; later changes to the
-- validity leave stored values as they are (Q3). Item completions are not recorded (Q5).
--
-- Columns beyond the data model: `org_id`; `course_id`, which the reads filter by; `expired_at`,
-- which spec 14 I2's expiry job sets. `target_id` has no foreign key (a course or a module by
-- `target_type`): a module deleted later leaves its completions as history.
CREATE TABLE completions (
    id            uuid        PRIMARY KEY,
    org_id        uuid        NOT NULL,
    user_id       uuid        NOT NULL,
    course_id     uuid        NOT NULL,
    target_type   text        NOT NULL CHECK (target_type IN ('course', 'module')),
    target_id     uuid        NOT NULL,
    cycle         integer     NOT NULL CHECK (cycle >= 1),
    completed_at  timestamptz NOT NULL,
    expires_at    timestamptz,
    expired_at    timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (user_id, target_type, target_id, cycle),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    CHECK (expires_at IS NULL OR expires_at > completed_at)
);
CREATE INDEX completions_due ON completions (expires_at) WHERE expired_at IS NULL AND expires_at IS NOT NULL;
CREATE INDEX completions_by_course ON completions (org_id, course_id, user_id);

ALTER TABLE courses ADD COLUMN completion_valid_months smallint CHECK (completion_valid_months BETWEEN 1 AND 120);
ALTER TABLE modules ADD COLUMN completion_valid_months smallint CHECK (completion_valid_months BETWEEN 1 AND 120);
