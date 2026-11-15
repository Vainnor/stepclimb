-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Local accounts' passkeys (docs/specs/m0/04-local-accounts.md B8, Data, Q3; plan, Technical ->
-- data model). `credential` is the credential webauthn-rs serializes: the public key, the sign
-- counter and the backup flags, no secret. `credential_id` is the id the authenticator chose,
-- copied out of it so one authenticator credential is registered at most once per organization
-- (the WebAuthn registration ceremony's "not yet registered" step) and so a sign-in can find it.
-- WebAuthn Level 3 caps a credential id at 1023 bytes.
--
-- The plan derives the organization through `user_id`; the table carries `org_id` as every
-- table but `organizations` does, and the composite foreign key keeps a passkey's user in the
-- passkey's organization.
CREATE TABLE passkeys (
    id             uuid        PRIMARY KEY,
    org_id         uuid        NOT NULL REFERENCES organizations (id),
    user_id        uuid        NOT NULL,
    credential_id  bytea       NOT NULL
        CHECK (octet_length(credential_id) BETWEEN 1 AND 1023),
    credential     jsonb       NOT NULL CHECK (jsonb_typeof(credential) = 'object'),
    name           text        NOT NULL CHECK (btrim(name) <> ''),
    last_used_at   timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id) ON DELETE CASCADE,
    UNIQUE (org_id, credential_id)
);
CREATE INDEX passkeys_by_user ON passkeys (org_id, user_id);
