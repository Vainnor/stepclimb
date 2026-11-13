-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Plans and each organization's plan (docs/specs/m0/10-entitlements-framework.md B3 to B6, Data).
-- A plan is an instance-wide row, not organization data. The server upserts the `unlimited` plan
-- from code at startup and never reads its `features` or `limits`: they are for SQL readers
-- (B4). Every organization has exactly one `org_plans` row, keyed by `org_id` (B5).
CREATE TABLE plans (
    id          uuid        PRIMARY KEY,
    key         text        NOT NULL UNIQUE,
    features    jsonb       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(features) = 'object'),
    limits      jsonb       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(limits) = 'object'),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- `ends_at` and `stripe_sub` are M6's; M0 neither writes nor reads them (B6).
CREATE TABLE org_plans (
    org_id      uuid        PRIMARY KEY REFERENCES organizations (id),
    plan_id     uuid        NOT NULL REFERENCES plans (id),
    source      text        NOT NULL CHECK (source IN ('stripe', 'grant', 'nonprofit', 'selfhost')),
    ends_at     timestamptz,
    stripe_sub  text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
