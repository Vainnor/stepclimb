-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Refresh tokens of the OAuth 2.1 server for Stepclimb's own apps, and what an authorization
-- code's exchange records (docs/specs/m0/08-sessions-and-csrf.md B32 to B35, Data, Q11).
--
-- A row is one refresh token, stored as the SHA-256 of its text (Q2), never the token. The
-- first token a code exchange issues starts a family: its `family_id` is its own `id` and it
-- rotated from nothing. Each refresh adds a row to the family with `rotated_from` naming the
-- token it replaced, which stays as the record that it was used: a token is spent when a row
-- rotated from it exists, and the unique index makes two concurrent rotations of one token
-- conflict. Deleting a family's first row deletes the family (`family_id` cascades). `org_id`
-- is direct, as on `oauth_authorization_codes`, so every query filters on the organization
-- without a join; the composite key keeps the user in the token's organization.
CREATE TABLE oauth_refresh_tokens (
    id            uuid        PRIMARY KEY,
    org_id        uuid        NOT NULL REFERENCES organizations (id),
    client_id     text        NOT NULL CHECK (client_id <> ''),
    user_id       uuid        NOT NULL,
    token_hash    bytea       NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    family_id     uuid        NOT NULL REFERENCES oauth_refresh_tokens (id) ON DELETE CASCADE,
    rotated_from  uuid        REFERENCES oauth_refresh_tokens (id) ON DELETE CASCADE,
    expires_at    timestamptz NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id) ON DELETE CASCADE,
    -- A family's first token rotated from nothing; every later one rotated from another.
    CONSTRAINT oauth_refresh_tokens_family_start CHECK ((id = family_id) = (rotated_from IS NULL))
);
CREATE INDEX oauth_refresh_tokens_family ON oauth_refresh_tokens (family_id);
CREATE UNIQUE INDEX oauth_refresh_tokens_rotated_once ON oauth_refresh_tokens (rotated_from);
CREATE INDEX oauth_refresh_tokens_expiry ON oauth_refresh_tokens (org_id, expires_at);

-- An exchanged code keeps when it was used and the family its exchange started, so a second
-- exchange of it is refused and revokes that family (B32). A revoked family leaves `family_id`
-- null; the code stays used.
ALTER TABLE oauth_authorization_codes
    ADD COLUMN used_at   timestamptz,
    ADD COLUMN family_id uuid REFERENCES oauth_refresh_tokens (id) ON DELETE SET NULL,
    ADD CONSTRAINT oauth_authorization_codes_family_when_used
        CHECK (family_id IS NULL OR used_at IS NOT NULL);
