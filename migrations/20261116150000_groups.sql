-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Groups and their members (docs/specs/m0/07-admission.md B18, B22, Q8 (a), I7). The plan's
-- `groups(id, org_id, key, name, source)` and `group_members(group_id, user_id, source)`, with
-- `org_id` on the members too so every row carries its organization, and the timestamps every
-- table has. M0 fills them at sign-in only: from a provider's group claim (`idp`) and from the
-- admission webhook's reply (`admission_webhook`); M4's sync API, SCIM, connectors and admins
-- add the other sources.
--
-- A membership is one row per giver: a source, and for `idp` the provider whose claim gave it
-- (`provider_id`, NULL for every other source). A group two providers or the webhook also return
-- is several rows, and each giver replaces only its own (Q8's same-source removal, per provider
-- for claims). Deleting a provider deletes its memberships. A group key is unique in its
-- organization and is matched exactly, without case folding. The key check refuses what
-- `stepclimb_core::groups::usable_key` refuses: the control characters (C0, DEL, C1), the
-- format characters (Unicode 16.0 general category Cf, the table of
-- `stepclimb_core::users::FORMAT_CHARS`) and the line and paragraph separators U+2028 and
-- U+2029; `text` cannot hold NUL. An organization has at most 10,000 groups
-- (`stepclimb_core::groups::MAX_GROUPS`), which the repository enforces.

CREATE TABLE groups (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    key         text        NOT NULL CHECK (key <> '' AND char_length(key) <= 255
                                            AND key !~ ('[\x01-\x1f\x7f-\x9f\u00ad\u0600-\u0605\u061c'
                                                        '\u06dd\u070f\u0890-\u0891\u08e2\u180e'
                                                        '\u200b-\u200f\u2028-\u202e\u2060-\u2064'
                                                        '\u2066-\u206f\ufeff\ufff9-\ufffb'
                                                        '\U000110bd\U000110cd\U00013430-\U0001343f'
                                                        '\U0001bca0-\U0001bca3\U0001d173-\U0001d17a'
                                                        '\U000e0001\U000e0020-\U000e007f]')),
    name        text        NOT NULL CHECK (name <> '' AND char_length(name) <= 255),
    source      text        NOT NULL CHECK (source IN ('local', 'idp', 'sync', 'scim', 'connector',
                                                       'admission_webhook')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (org_id, key)
);

CREATE TABLE group_members (
    org_id       uuid        NOT NULL,
    group_id     uuid        NOT NULL,
    user_id      uuid        NOT NULL,
    source       text        NOT NULL CHECK (source IN ('local', 'idp', 'sync', 'scim', 'connector',
                                                        'admission_webhook')),
    provider_id  uuid,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    CHECK ((source = 'idp') = (provider_id IS NOT NULL)),
    UNIQUE NULLS NOT DISTINCT (group_id, user_id, source, provider_id),
    FOREIGN KEY (org_id, group_id) REFERENCES groups (org_id, id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, provider_id) REFERENCES identity_providers (org_id, id) ON DELETE CASCADE
);
CREATE INDEX group_members_by_user ON group_members (org_id, user_id, source, provider_id);
