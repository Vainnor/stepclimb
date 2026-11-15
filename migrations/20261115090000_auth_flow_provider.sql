-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The provider a sign-in flow belongs to (docs/specs/m0/08-sessions-and-csrf.md Data,
-- `auth_flows`; docs/specs/m0/05-oidc.md B5, B7). An OIDC or OAuth 2 flow names the provider it
-- was started for, so its callback can refuse a flow started for another provider. Other kinds
-- leave it NULL. The composite foreign key keeps the provider in the flow's organization, and
-- deleting a provider deletes its open flows.
ALTER TABLE auth_flows
    ADD COLUMN provider_id uuid,
    ADD FOREIGN KEY (org_id, provider_id) REFERENCES identity_providers (org_id, id)
        ON DELETE CASCADE;
