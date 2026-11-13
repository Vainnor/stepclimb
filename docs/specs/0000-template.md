# <Spec title>

- **Status:** Draft | Ready | Implemented
- **Milestone:** M0 | M1 | ...
- **Plan:** <`docs/plan/...` sections this spec implements>
- **Depends on:** <other specs>
- **Review:** <"maintainer line-by-line" when it touches auth, sessions, permissions, grading,
  feedback visibility, billing or deletion; otherwise "standard">

## Summary

Two or three sentences: what this delivers and for whom.

## Behavior

What the system does, numbered so tests and issues can cite it (B1, B2, ...). Every rule the
plan states, and nothing the plan leaves open (that goes under Open questions).

## API

Endpoints under `/api/v1` (or the plan's `/auth`, `/oauth`, `/.well-known` paths): method, path,
request, response, errors (`application/problem+json` with a stable `code`), permission. Each is
in the OpenAPI document.

## Data

Tables and columns (as the plan's data model names them), constraints, indexes, migrations.
Every table but `organizations` carries `org_id`.

## Permissions

The permission catalog entries this spec adds, which built-in roles hold them, and at which scope.
Entitlement features it checks, if any.

## Test cases

Numbered (T1, T2, ...), each naming the behavior it proves. Every protected route: the four-case
permission test (401, 403, 404, 2xx) and the cross-organization test.

## Out of scope

What this spec does not do, and where it is done instead.

## Open questions

Points the plan leaves open, for the owner. Each with the options and a recommendation; none is
decided by implementing it.

## Issues

The PR-sized issues this spec breaks into, in order, each with what it delivers and its depends-on.
