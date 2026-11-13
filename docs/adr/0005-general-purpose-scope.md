# 0005. General-purpose scope, with connectors for VATSIM

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/product.md`; `docs/plan/technical.md` -> Goals, Integration platform, vZDC deployment

## Context

vZDC is the first deployment, but the plan targets any organization: companies, clubs,
schools and VATSIM facilities.

## Decision

Stepclimb is a general-purpose LMS for any organization, VATSIM or not. VATSIM and VATUSA are
optional connectors on the public API; the ATC activities are an optional pack. Nothing in the core
assumes VATSIM or aviation. The vZDC website needs no changes; it can integrate through the APIs
if it wants.

## Consequences

`bin/check-layering.sh` keeps `stepclimb-connectors` off every workspace crate and lets only the
server depend on the connectors and the ATC pack. Everything vZDC-specific lives in vZDC's
configuration and data sets.
