# 0010. Feedback settings enforced by the server

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> Feedback settings; `docs/plan/vzdc-brief.md` -> What training staff need

## Context

Practice should explain every answer; tests and exams must not reveal results early.

## Decision

Every quiz and activity has two settings, overridable per student or group: during the attempt
(full, hint, result only, none) and after submitting (nothing, score, right/wrong, full review),
with a release rule (immediately, after the close date, or manually by staff). Presets: Practice,
Quiz, Exam. The server enforces both: the API returns only what the setting allows. Trainers
always see everything.

## Consequences

Every answer, step and review response passes through one filter per response type, tested for
each setting (`assessment-integrity.md`).
