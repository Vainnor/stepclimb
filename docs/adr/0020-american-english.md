# 0020. American English in docs, comments and UI copy

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, answering the Phase 1 question on 2026-11-12
- **Related:** `.claude/rules/engineering-prose.md`; `docs/plan/README.md`

## Context

Mainline's prose rules require American English; the plan is written in British English
(organisation, licence, colour). The rules and the plan disagreed.

## Decision

Stepclimb's docs, comments, commit messages and UI copy use American English. The plan's text is
converted when it is brought into the repository (`docs/plan/`); identifiers use the names the plan
gives (`organizations`, `enrollments`).

## Consequences

`docs/plan/` is the plan in American spelling; the artifact keeps its own spelling.
