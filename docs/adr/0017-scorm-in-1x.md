# 0017. SCORM playback in 1.x

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> Goals and non-goals, Milestones

## Context

Some organizations have SCORM packages; supporting them before 1.0 would delay the pilot.

## Decision

SCORM 1.2 and 2004 playback ships in 1.x, in a sandboxed frame. The data model leaves room for it.

## Consequences

No SCORM work before 1.0; `module_items.kind` keeps room for a future item kind.
