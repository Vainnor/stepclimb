-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Groups and their members (docs/specs/m0/07-admission.md B18, B22, Q8 (a), I7). The plan's
-- `groups(id, org_id, key, name, source)` and `group_members(group_id, user_id, source)`, with
-- `org_id` on the members too so every row carries its organization, and the timestamps every
-- table has. M0 fills them at sign-in only: from a provider's group claim (`idp`) and from the
-- admission webhook's reply (`admission_webhook`); M4's sync API, SCIM, connectors and admins
-- add the other sources.
--
-- A membership is one row per source, so a group the claims and the webhook both return is two
-- rows, and each source replaces only its own (Q8's same-source removal). A group key is
-- unique in its organization and is matched exactly, without case folding. The key check refuses
-- the Unicode control characters (C0, DEL and C1), the set `stepclimb_core::groups::usable_key`
-- refuses; `text` cannot hold NUL.

CREATE TABLE groups (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    key         text        NOT NULL CHECK (key <> '' AND char_length(key) <= 255
                                            AND key !~ '[\x01-\x1f\x7f-\x9f]'),
    name        text        NOT NULL CHECK (name <> '' AND char_length(name) <= 255),
    source      text        NOT NULL CHECK (source IN ('local', 'idp', 'sync', 'scim', 'connector',
                                                       'admission_webhook')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (org_id, key)
);

CREATE TABLE group_members (
    org_id      uuid        NOT NULL,
    group_id    uuid        NOT NULL,
    user_id     uuid        NOT NULL,
    source      text        NOT NULL CHECK (source IN ('local', 'idp', 'sync', 'scim', 'connector',
                                                       'admission_webhook')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, user_id, source),
    FOREIGN KEY (org_id, group_id) REFERENCES groups (org_id, id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id)
);
CREATE INDEX group_members_by_user ON group_members (org_id, user_id, source);
