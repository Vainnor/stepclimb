-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The local provider (docs/specs/m0/04-local-accounts.md B1, B3). Every organization has one
-- `identity_providers` row with kind `local` and key `local`; local accounts' identities point at
-- it. This widens the kind check of docs/specs/m0/03-users-and-identities.md B13, and ties the
-- kind and the key together, so no OpenID Connect or OAuth 2 provider can take the key `local`
-- (whose `/auth/local/...` routes are the local methods') and the local row has no other key.
-- `UNIQUE (org_id, key)` then allows at most one local row per organization.
--
-- The rows themselves are written from code, with UUID v7 keys: `organizations::create` for a
-- new organization, and `stepclimb serve` at startup for every organization without one.
ALTER TABLE identity_providers DROP CONSTRAINT identity_providers_kind_check;
ALTER TABLE identity_providers ADD CONSTRAINT identity_providers_kind_check
    CHECK (kind IN ('oidc', 'oauth2', 'local'));
ALTER TABLE identity_providers ADD CONSTRAINT identity_providers_local_key
    CHECK ((kind = 'local') = (key = 'local'));
