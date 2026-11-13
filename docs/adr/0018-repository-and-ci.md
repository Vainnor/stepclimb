# 0018. Repository setup and CI

- **Status:** Accepted
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
  Biome, tsc, Vitest, the Playwright smoke test against Compose, and `just agents-check`. GitHub
  Actions runs the same recipe on Linux for every PR and push.
- The Tauri build runs on Linux for every PR and on macOS and Windows for pushes to `main`, on
  standard hosted runners (owner, 2026-11-12). Paid or larger runners need the owner.
- Dependabot opens weekly update PRs, held to versions at least 14 days old, as the plan's
  supply-chain list says; pnpm refuses anything younger (`minimumReleaseAge`).
- `main` holds releases, `next` is the integration branch, squash merges only, PR only.
- Framework pins follow the plan's versions where it names one: axum 0.8, sqlx 0.9, utoipa 5
  (utoipa 6.0.0 exists; moving to it is a new ADR).
- The GitHub repository is private, `Vainnor/stepclimb`, created on 13 Nov 2026 with squash
  merges only and branches deleted on merge; `next` is the default branch.

## Consequences

Branch protection is enforced on GitHub. Ruleset "main and next: PR only" (id 27175641, created
13 Nov 2026) applies to `main` and `next`: no deletion, no force push, a pull request for every
change (squash only, conversations resolved, no required approvals because there is one
maintainer), and the `ci` and `build (ubuntu-24.04)` checks required and up to date. Nobody can
bypass it, the owner included; `gh api repos/Vainnor/stepclimb/rules/branches/next` lists the four
rules. `.claude/hooks/git-guard` stays as the local guard, as in Mainline.

GitHub's private vulnerability reporting is not available while the repository is private (its
API answers 404), so `SECURITY.md` asks for direct contact until the repository is public.
