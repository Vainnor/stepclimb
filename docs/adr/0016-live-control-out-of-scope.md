# 0016. Live control simulation is out of scope

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> Interactive activities (Out of scope)

## Context

ATC training has knowledge and procedure on one side and live traffic control on the other.

## Decision

Live control simulation (vectoring to final, arrival merging, conflict resolution, reroutes,
handoffs) is practiced in the real ATC tools during actual training, not in Stepclimb. The ATC pack
covers knowledge and procedure: flight plans, headings, clearances, phraseology and commands. The
ERAM/CRC command drill is the only enroute game.

## Consequences

No radar simulation, traffic model or pilot AI enters the codebase.
