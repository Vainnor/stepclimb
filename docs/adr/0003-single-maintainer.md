# 0003. A single maintainer

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> License and governance, How it's built

## Context

Stepclimb is an independent project, not commissioned by vZDC. AI coding agents write most of
the code; the maintainer directs, reviews and tests it.

## Decision

One maintainer. Outside contributions are welcome through pull requests, but there is no plan to
add maintainers. The maintainer reviews authentication, sessions, permissions, grading, feedback
visibility, billing and data deletion line by line.

## Consequences

Review time is the pace-setter: specs come first, gates do the checking, and PRs stay small.
The agent setup (`AGENTS.md`, `CLAUDE.md`, `.claude/`) exists to make agent-written code safe to
merge. The bus-factor risk is mitigated by public code, documentation and design records (plan,
vZDC brief -> Risks).
