-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Sign-in flows (docs/specs/m0/08-sessions-and-csrf.md B17 to B21, Q8): the server-side state of
-- a sign-in that spans more than one request, bound to the browser by the `__Host-sc_flow`
-- cookie. `org_id` is direct because a flow exists before any user is known. `token_hash` is the
-- SHA-256 of the cookie's token, never the token. `payload` is AES-256-GCM sealed under the key
-- ring, bound to (org_id, 'auth_flows', id, 'payload'). The `provider_id` column of the spec's
-- Data section arrives with the first flow that names a provider, once `identity_providers`
-- exists.
CREATE TABLE auth_flows (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    token_hash  bytea       NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    kind        text        NOT NULL CHECK (kind IN ('oidc', 'oauth2', 'passkey_register',
                                                     'passkey_sign_in', 'password_totp',
                                                     'oauth_authorize')),
    user_id     uuid,
    payload     bytea       NOT NULL,
    -- B19: a path on this host. Printable ASCII, starting with one `/` not followed by `/` or
    -- `\`. The repository only stores values that already pass the same rule.
    return_to   text        NOT NULL CHECK (
                                return_to ~ '^/([^/\\]|$)'
                                AND return_to ~ '^[!-~]+$'
                                AND octet_length(return_to) <= 2048
                            ),
    expires_at  timestamptz NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    -- The user is in the flow's organization: the composite key cannot point across tenants.
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id) ON DELETE CASCADE
);
CREATE INDEX auth_flows_expiry ON auth_flows (org_id, expires_at);
