-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The admission webhook (docs/specs/m0/07-admission.md B13, B24, Data, Q2, Q7): at most one per
-- organization, which admission calls at sign-in (spec 07 issue 6). `secret` is the Standard
-- Webhooks signing secret sealed with the key ring (spec 08 B3), bound to the organization, this
-- table, the row's id and the column, never the secret itself. `url` is kept as the admin wrote
-- it; the server checks it on save and again at every connection. A new webhook is off until an
-- admin enables it, so the receiver can be given the secret first.
CREATE TABLE admission_webhooks (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL UNIQUE REFERENCES organizations (id),
    url         text        NOT NULL CHECK (url <> '' AND char_length(url) <= 2048),
    secret      bytea       NOT NULL CHECK (octet_length(secret) > 0),
    enabled     boolean     NOT NULL DEFAULT false,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
