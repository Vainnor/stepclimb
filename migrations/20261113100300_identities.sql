-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Identity providers and identities (docs/specs/m0/03-users-and-identities.md B13 to B16, Data).
-- A provider's `config` shape, and the encryption of the secrets inside it, belong to
-- docs/specs/m0/05-oidc.md and 06-oauth2-and-vatsim-preset.md. `UNIQUE (org_id, id)` is the
-- target of the composite foreign keys that keep an identity, and later a sign-in flow, in its
-- provider's organization.
CREATE TABLE identity_providers (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    key         text        NOT NULL,
    kind        text        NOT NULL CHECK (kind IN ('oidc', 'oauth2')),
    config      jsonb       NOT NULL CHECK (jsonb_typeof(config) = 'object'),
    enabled     boolean     NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (org_id, key)
);

-- An identity links one user to one provider by the provider's subject. `(provider_id, subject)`
-- is unique, so one external account maps to at most one user (B14). The composite foreign keys
-- make an identity whose user or provider is in another organization impossible.
CREATE TABLE identities (
    id           uuid        PRIMARY KEY,
    org_id       uuid        NOT NULL,
    user_id      uuid        NOT NULL,
    provider_id  uuid        NOT NULL,
    subject      text        NOT NULL CHECK (subject <> ''),
    attributes   jsonb       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(attributes) = 'object'),
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, provider_id) REFERENCES identity_providers (org_id, id),
    UNIQUE (provider_id, subject)
);
CREATE INDEX identities_by_user ON identities (org_id, user_id);
