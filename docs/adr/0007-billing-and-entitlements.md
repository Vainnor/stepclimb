# 0007. Billing and entitlements; self-hosted gets everything

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> Plans and billing

## Context

Managed hosting needs plans that gate features and limits, without making the self-hosted
edition a crippled one.

## Decision

A plan is a set of feature flags and limits, checked in code with a `Require<Feature>` extractor
next to the permission checks. The framework exists from M0 with everything enabled. Gating applies
only on managed hosting; a self-hosted instance runs an unlimited plan. Billing uses Stripe
(Checkout, Customer Portal, Tax); downgrades never delete data. A free non-profit plan is granted
after an application; the operator can grant any plan without billing.

## Consequences

M0 builds the entitlements framework (`docs/specs/m0/10-entitlements-framework.md`) with no
gating; M6 adds plans, Stripe and the operator console. Plan tiers and prices are deferred to M6.
