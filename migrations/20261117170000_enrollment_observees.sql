-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The learners an Observer enrollment observes (docs/specs/m1/05-progress-and-events.md B17 to
-- B20, Q4, Data). Not in the plan's data model: the Observer's "named students" (m0/09 B5) are
-- recorded per course, on the Observer's enrollment.
--
-- One row per Observer enrollment and observed learner. The repository checks that the
-- enrollment's role holds `progress.view_observed` and that each user is a learner of the
-- course (B17). The composite foreign key to `enrollments` keeps `course_id` the enrollment's
-- course and `org_id` its organization, so a row cannot point a course's Observer at another
-- course's learners; it cascades, so deleting the Observer's enrollment deletes its links. The
-- links that name a learner are deleted by the repository when the learner's last learner
-- enrollment in the course is deleted (B15, B20).
ALTER TABLE enrollments
    ADD CONSTRAINT enrollments_org_id_course_id_id_key UNIQUE (org_id, course_id, id);

CREATE TABLE enrollment_observees (
    org_id         uuid        NOT NULL,
    enrollment_id  uuid        NOT NULL,
    course_id      uuid        NOT NULL,
    user_id        uuid        NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (enrollment_id, user_id),
    FOREIGN KEY (org_id, course_id, enrollment_id)
        REFERENCES enrollments (org_id, course_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id)
);
CREATE INDEX enrollment_observees_by_user ON enrollment_observees (org_id, course_id, user_id);
