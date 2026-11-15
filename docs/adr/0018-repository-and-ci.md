# 0018. Repository setup and CI

- **Status:** Accepted (CI amended by 0021, release merges by 0030)
- **Date:** 2026-11-12
- **Decider:** the owner: the plan's Decided list (Repo setup), and the owner's answers on 2026-11-12
- **Related:** `docs/plan/technical.md` -> Decided (Repo setup), Testing, How it's built; `justfile`; `.github/`

## Context

The plan decides a Cargo and pnpm workspace monorepo with CI gates, protected `main`, squash
merges, a GitHub Project board and decisions in `docs/adr/`. The owner settled the remaining
setup choices on 12 Nov 2026.

## Decision

- A Cargo and pnpm workspace monorepo; `just` is the command runner with Mainline's recipe names
  where they apply; a Nix dev shell (`flake.nix`) mirrors Mainline's.
- `just ci` is the merge gate: gitleaks, `cargo fmt --check`, clippy `-D warnings`, the layering
  check, SPDX headers, nextest plus doctests, cargo-deny, the sqlx offline check, the OpenAPI diff,
  Biome, tsc, Vitest, the Playwright smoke test against Compose, and `just agents-check`. It runs
  locally; ADR 0021 replaces the hosted CI this ADR first described.
- Dependencies are updated by hand once a month and before each release, as in Mainline, held to
  versions at least 14 days old (pnpm refuses anything younger, `minimumReleaseAge`). The plan's
  supply-chain list names Dependabot; the owner chose manual updates instead on 13 Nov 2026.
- `main` holds releases, `next` is the integration branch, squash merges only, PR only.
- Framework pins follow the plan's versions where it names one: axum 0.8, sqlx 0.9, utoipa 5
  (utoipa 6.0.0 exists; moving to it is a new ADR).
- The GitHub repository is private, `Vainnor/stepclimb`, created on 13 Nov 2026 with squash
  merges only and branches deleted on merge; `next` is the default branch.

## Consequences

Branch protection is enforced on GitHub. Ruleset "main and next: PR only" (id 27175641, created
13 Nov 2026) applies to `main` and `next`: no deletion, no force push, a pull request for every
change (squash only, conversations resolved, no required approvals because there is one
maintainer). It first also required the `ci` and `build (ubuntu-24.04)` checks; that rule was
removed on 13 Nov 2026 when hosted CI was dropped (ADR 0021), since nothing posts those checks.
Nobody can bypass the ruleset, the owner included; `gh api
repos/Vainnor/stepclimb/rules/branches/next` lists its rules. `.claude/hooks/git-guard` stays as the local guard, as in Mainline.

GitHub's private vulnerability reporting is not available while the repository is private (its
API answers 404), so `SECURITY.md` asks for direct contact until the repository is public.
