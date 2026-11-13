-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Browser sessions (docs/specs/m0/08-sessions-and-csrf.md B7 to B11). The organization comes
-- through `user_id` -> `users.org_id`; every query joins `users` and filters on the OrgScope.
-- `token_hash` is the SHA-256 of the cookie's token (Q2), never the token itself: the length
-- check refuses the 43-character token text.
CREATE TABLE sessions (
    id            uuid        PRIMARY KEY,
    user_id       uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash    bytea       NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    kind          text        NOT NULL CHECK (kind IN ('browser')),
    last_seen_at  timestamptz NOT NULL,
    expires_at    timestamptz NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_id ON sessions (user_id);
