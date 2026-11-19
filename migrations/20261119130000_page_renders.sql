-- SPDX-License-Identifier: AGPL-3.0-or-later
-- A page version rendered to HTML (docs/specs/m1/07-pages-and-the-editor.md B15): written on the
-- first read of a version and written again when the renderer's version in code is higher.
-- Images are placeholders (`data-sc-image="<course_file_id>"`) that each read fills with a signed
-- URL, so no row holds a credential. Not in the data model.
CREATE TABLE page_renders (
    org_id            uuid        NOT NULL,
    version_id        uuid        PRIMARY KEY,
    renderer_version  integer     NOT NULL CHECK (renderer_version >= 1),
    html              text        NOT NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, version_id) REFERENCES page_versions (org_id, id) ON DELETE CASCADE
);
