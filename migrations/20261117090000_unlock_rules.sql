-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Unlock and completion rules (docs/specs/m1/04-unlock-and-completion-rules.md B1 to B3, Data;
-- plan, Technical -> data model `modules`, `module_prerequisites`, `module_items.requirement`).
--
-- A module may wait for `unlock_at`, for its prerequisite modules, or both; `require_sequential`
-- puts its items in order, and `completion_rule` says whether all or one of its requirements
-- complete it (Q1). An item carries at most one `requirement`, whose shape the typed
-- `stepclimb_core::items::Requirement` checks; the column refuses only a non-object.
--
-- `org_id` and `course_id` on `module_prerequisites` (the data model lists `module_id,
-- requires_id`) let the two foreign keys hold both modules to one course of one organization.
-- Deleting a module removes the rows that name it (B2). Cycles are refused by the repository
-- under the course lock, which a constraint cannot express.
ALTER TABLE modules
    ADD COLUMN unlock_at          timestamptz,
    ADD COLUMN require_sequential boolean NOT NULL DEFAULT false,
    ADD COLUMN completion_rule    text    NOT NULL DEFAULT 'all' CHECK (completion_rule IN ('all', 'one'));

ALTER TABLE module_items
    ADD COLUMN requirement jsonb CHECK (requirement IS NULL OR (jsonb_typeof(requirement) = 'object' AND requirement ? 'kind')),
    ADD CONSTRAINT module_items_header_no_requirement CHECK (kind <> 'header' OR requirement IS NULL);

CREATE TABLE module_prerequisites (
    org_id       uuid        NOT NULL,
    course_id    uuid        NOT NULL,
    module_id    uuid        NOT NULL,
    requires_id  uuid        NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (module_id, requires_id),
    FOREIGN KEY (org_id, course_id, module_id)   REFERENCES modules (org_id, course_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, course_id, requires_id) REFERENCES modules (org_id, course_id, id) ON DELETE CASCADE,
    CHECK (module_id <> requires_id)
);
CREATE INDEX module_prerequisites_requires ON module_prerequisites (requires_id);
