-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Assignments and their assignees' rows
-- (docs/specs/m1/13-assignments-due-dates-reminders-and-escalation.md B1 to B9, Data; plan,
-- Technical -> data model `assignments`, `reminders_sent`).
--
-- An assignment names a course, module or item of one course (`target_type`, `target_id`) and
-- who must finish it (`assignee_type`, `assignee_id`), with an optional due rule: a fixed time
-- or a number of days after each person was assigned (B4). Each person it reaches gets an
-- `assignment_users` row with their own `assigned_at`, `due_at` and `completed_at`; the status
-- is computed when read (B7). `target_id` and `assignee_id` point at different tables by type,
-- as `role_assignments.scope_id` does, so they have no foreign key: the repository checks both
-- in the organization and the course, and refuses to delete an assigned module or item (B1,
-- Data). `assignments_by_target` serves that check.
--
-- `reminders_sent` (spec 13 I3) and `assignment_events` (I4) arrive with the issues that write
-- them, with composite `(org_id, assignment_user_id)` keys to `assignment_users`, which
-- `UNIQUE (org_id, id)` here lets them reference.
CREATE TABLE assignments (
    id             uuid        PRIMARY KEY,
    org_id         uuid        NOT NULL,
    course_id      uuid        NOT NULL,
    target_type    text        NOT NULL CHECK (target_type IN ('course', 'module', 'item')),
    target_id      uuid        NOT NULL,
    assignee_type  text        NOT NULL CHECK (assignee_type IN ('user', 'group')),
    assignee_id    uuid        NOT NULL,
    due_fixed      timestamptz,
    due_relative   integer     CHECK (due_relative BETWEEN 1 AND 3650),
    recurrence     jsonb,
    reminder_rule  jsonb       NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(reminder_rule) = 'array'),
    cycle          integer     NOT NULL DEFAULT 1,
    created_by     uuid        NOT NULL,
    ended_at       timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, created_by) REFERENCES users (org_id, id),
    CHECK (due_fixed IS NULL OR due_relative IS NULL)
);
CREATE INDEX assignments_by_course ON assignments (org_id, course_id) WHERE ended_at IS NULL;
CREATE INDEX assignments_by_target ON assignments (org_id, target_type, target_id);

CREATE TABLE assignment_users (
    id             uuid        PRIMARY KEY,
    org_id         uuid        NOT NULL,
    assignment_id  uuid        NOT NULL,
    user_id        uuid        NOT NULL,
    cycle          integer     NOT NULL DEFAULT 1 CHECK (cycle >= 1),
    assigned_at    timestamptz NOT NULL,
    due_at         timestamptz,
    completed_at   timestamptz,
    removed_at     timestamptz,
    next_step_at   timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (assignment_id, user_id, cycle),
    FOREIGN KEY (org_id, assignment_id) REFERENCES assignments (org_id, id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id)
);
CREATE INDEX assignment_users_next_step ON assignment_users (next_step_at)
    WHERE next_step_at IS NOT NULL AND completed_at IS NULL AND removed_at IS NULL;
CREATE INDEX assignment_users_by_user ON assignment_users (org_id, user_id);
