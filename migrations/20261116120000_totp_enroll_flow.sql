-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The `password_totp_enroll` sign-in flow (docs/specs/m0/04-local-accounts.md B15): a right
-- password for an account without TOTP, while the organization requires TOTP of password
-- accounts, waits for the user to set up TOTP before any session exists. It is a kind of its
-- own so that neither the code step's flow nor this one can stand in for the other.
ALTER TABLE auth_flows DROP CONSTRAINT auth_flows_kind_check;
ALTER TABLE auth_flows ADD CONSTRAINT auth_flows_kind_check
    CHECK (kind IN ('oidc', 'oauth2', 'passkey_register', 'passkey_sign_in', 'password_totp',
                    'oauth_authorize', 'password_totp_enroll'));
