# 0015. The UI library lives in the repository only

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> License and governance; `docs/plan/ui.md` -> UI decisions

## Context

Publishing a component library adds versioning, support and compatibility work for one
maintainer.

## Decision

`packages/ui` and `packages/tokens` stay in the repository. They are not published to npm or a
shadcn registry. Versions are managed with Changesets inside the repo, and breaking changes to
stable components ship with a codemod.

## Consequences

Both packages are `"private": true`. Adapted MIT code keeps its notice in
`packages/ui/THIRD_PARTY.md`.
