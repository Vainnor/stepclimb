---
paths:
  - "crates/stepclimb-assess/**"
  - "crates/stepclimb-activities/**"
  - "crates/stepclimb-activities-atc/**"
---
# Assessment Integrity Rules (not loaded at startup; read in full before writing or reviewing grading, question types, activities or their data)

Trainers rely on these results, and an organization's training records may depend on them. A
result that is wrong, or an answer that leaks, is the worst bug this project can ship. The plan's
sections are Technical -> Assessments, Interactive activities, and Feedback settings.

---

## Grading runs on the server

- `stepclimb-assess` grades only on the server. Its wasm32 build (`packages/assess-wasm`) is for
  instructors' authoring previews and ungraded practice, where the answers are not secret, and
  never produces a grade that is stored.
- A `QuestionType::Render` and an `Activity::Problem` **never contain the answer or the
  solution**. `Activity::Solution` stays on the server. A property test asserts this for every
  type: serialize the render or problem and search it for the key.

## Seeded determinism

- Every random choice (shuffle, draw, generated problem) comes from a seed stored with the attempt
  or session, through the seeded RNG passed in. No `thread_rng`, no wall-clock time, no iteration
  over a `HashMap` that changes results.
- The same seed, definition version and data versions reproduce the same render, problem and
  grade on any machine; a test replays a recorded case to prove it.
- Attempts pin the exact question version shown; sessions record activity and data-set versions.

## Validate before serving

- Every generated problem passes `Activity::validate` before it is served; a problem that fails is
  discarded and logged with its seed, never shown.
- `check` accepts any valid answer by checking rules, not one expected string.
- Property tests run `generate -> validate -> check(solution)` across many seeds for every
  activity type.

## Feedback settings are enforced by the server

- The response to an answer, a step or a review contains only what the effective feedback setting
  allows (during: full, hint, result only, none; after: nothing, score, right/wrong, full review;
  and the release rule). Overrides per student or group apply first.
- The filtering happens in one function per response type, covered by a test for every setting:
  the payload under `none` carries no correctness, under `result only` no explanation or answer.
- Trainers always get everything; that is a separate, permission-checked endpoint.

## Regrading and audit

- Every grade change (regrade after a key correction, override, data-set fix) writes an
  `audit_events` row with the actor, the old and new values and the reason.
- A regrade reproduces the original render from its pinned version and seed; it never re-draws.

## Data sets and known cases

- Organization data sets (routes, altitude rules, heading tables, grammars) are validated on
  upload and versioned. The known-case library (YAML: input -> expected verdict, written with the
  TA) runs in CI and on every data-set change; a failing case blocks publishing.
- Nothing vZDC-specific is hard-coded in `stepclimb-activities-atc`: facility rules come from the
  data sets.
