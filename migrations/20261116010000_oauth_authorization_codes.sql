-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Authorization codes of the OAuth 2.1 server for Stepclimb's own apps
-- (docs/specs/m0/08-sessions-and-csrf.md B31, Data, Q10). A row is one code the authorization
-- endpoint issued after the user confirmed: bound to the organization, the user, the client,
-- the exact redirect URI and the PKCE S256 challenge, and valid for 60 seconds. `code_hash` is
-- the SHA-256 of the code's text (Q2), never the code itself. The token endpoint (spec 08 I8)
-- exchanges a code once and adds what it needs to record the exchange.
CREATE TABLE oauth_authorization_codes (
    id              uuid        PRIMARY KEY,
    org_id          uuid        NOT NULL REFERENCES organizations (id),
    user_id         uuid        NOT NULL,
    client_id       text        NOT NULL CHECK (client_id <> ''),
    code_hash       bytea       NOT NULL UNIQUE CHECK (octet_length(code_hash) = 32),
    redirect_uri    text        NOT NULL CHECK (redirect_uri <> ''),
    -- RFC 7636 section 4.2: BASE64URL(SHA-256(verifier)) without padding.
    code_challenge  text        NOT NULL CHECK (code_challenge ~ '^[A-Za-z0-9_-]{43}$'),
    expires_at      timestamptz NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    -- The user is in the code's organization: the composite key cannot point across tenants.
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id) ON DELETE CASCADE
);
CREATE INDEX oauth_authorization_codes_expiry ON oauth_authorization_codes (org_id, expires_at);
