# 0009. Grading on the server only

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> Assessments, Interactive activities; `.claude/rules/assessment-integrity.md`

## Context

Results must be trusted by trainers and may feed training records. A client that grades can be
made to lie, and a client that holds the answer key can leak it.

## Decision

Grading runs only on the server. Answer keys and solutions never reach a client before the
review policy allows. `stepclimb-assess` is pure Rust; its WASM build is used only for instructors'
authoring previews and for ungraded practice, where the answers are not secret. Deadlines are
stored and enforced on the server; the client only displays a countdown.

## Consequences

Question renders and activity problems are separate types from definitions and solutions, and
property tests assert renders never contain answers. Every grade change is audited.
