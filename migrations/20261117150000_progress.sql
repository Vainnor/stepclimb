-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Learner progress (docs/specs/m1/05-progress-and-events.md B3, B6, B7, Data; plan, Technical ->
-- data model `item_progress`, `module_progress / course_progress`).
--
-- One `item_progress` row per learner and item, created `started` at the first step and set
-- `completed` once the item's requirement is met; completion is final (Q1). `module_progress`
-- and `course_progress` store the roll-up counts and `completed_at`, written in the transaction
-- that completes an item, so reads do not recompute them.
--
-- Columns beyond the data model: `org_id` everywhere; `course_id` on `item_progress`, which the
-- roll-up and enrollment deletion read by course (B6, B15); `started_at`; the counts (B7).
-- `score` stays null until M2 writes it.
--
-- The `item_progress` foreign key to `module_items` has the default NO ACTION, which backs B16:
-- an item with progress cannot be deleted. `module_progress` cascades when its module is
-- deleted, which spec 03 B3 allows only for an empty module, so no item progress is lost.
CREATE TABLE item_progress (
    org_id        uuid        NOT NULL,
    user_id       uuid        NOT NULL,
    course_id     uuid        NOT NULL,
    item_id       uuid        NOT NULL,
    state         text        NOT NULL CHECK (state IN ('started', 'completed')),
    score         numeric,
    started_at    timestamptz NOT NULL,
    completed_at  timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, item_id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, item_id) REFERENCES module_items (org_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    CHECK ((state = 'completed') = (completed_at IS NOT NULL))
);
CREATE INDEX item_progress_by_course ON item_progress (org_id, course_id, user_id);
CREATE INDEX item_progress_by_item ON item_progress (item_id);

CREATE TABLE module_progress (
    org_id           uuid        NOT NULL,
    user_id          uuid        NOT NULL,
    course_id        uuid        NOT NULL,
    module_id        uuid        NOT NULL,
    required_count   integer     NOT NULL CHECK (required_count >= 0),
    completed_count  integer     NOT NULL CHECK (completed_count >= 0),
    completed_at     timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, module_id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, course_id, module_id) REFERENCES modules (org_id, course_id, id) ON DELETE CASCADE
);
CREATE INDEX module_progress_by_course ON module_progress (org_id, course_id, user_id);

CREATE TABLE course_progress (
    org_id           uuid        NOT NULL,
    user_id          uuid        NOT NULL,
    course_id        uuid        NOT NULL,
    required_count   integer     NOT NULL CHECK (required_count >= 0),
    completed_count  integer     NOT NULL CHECK (completed_count >= 0),
    completed_at     timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, course_id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id)
);
CREATE INDEX course_progress_by_course ON course_progress (org_id, course_id);
