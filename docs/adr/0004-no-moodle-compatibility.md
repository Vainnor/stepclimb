# 0004. No Moodle compatibility

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> Goals and non-goals, Integration platform; `docs/plan/vzdc-brief.md`

## Context

vZDC's current courses live in the VATUSA Moodle. Compatibility would shape Stepclimb's data
model around Moodle's.

## Decision

Moodle compatibility is not a goal. No import from the VATUSA Moodle is planned; courses are
rebuilt by staff in Stepclimb. Compatibility formats (GIFT, Moodle XML, QTI, Common Cartridge) are
optional extras after 1.0 and never constrain Stepclimb's own data model. Canvas and Moodle are
UX references only; no code is copied or translated from either.

## Consequences

`stepclimb-interop` holds the import and export formats when they come. `content-and-ip.md`
forbids copying Canvas or Moodle code.
