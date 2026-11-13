# 0008. Email senders: platform and organization

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> Email and notifications

## Context

Some emails are Stepclimb's (sign-up, billing, security notices); most are an organization's to
its people (reminders, escalations, grade releases).

## Decision

Platform emails come from Stepclimb's own sender. Organization emails come from the
organization's own domain (verified with DKIM and SPF) or its own SMTP server; until that is set
up they go out as "<Org> via Stepclimb" and admins are prompted to finish setup. Every organization
email has an editable template, the organization's branding, and respects each user's
notification preferences.

## Consequences

`email_senders` is in the data model; sending lands in M1 with `stepclimb-jobs`.
