# 0021. No hosted CI: the merge gate runs locally

- **Status:** Accepted
- **Date:** 2026-11-13
- **Decider:** the owner, 2026-11-13
- **Related:** ADR 0018; `.claude/rules/cross-platform.md`; Mainline's ADR 0074 (the same decision)

## Context

ADR 0018 had GitHub Actions run `just ci` on every PR and build the Tauri app on Linux, macOS and
Windows. The repository is private, and Actions minutes on private repositories run out fast
(macOS minutes count ten times). Mainline, the owner's other project, runs no hosted CI for the
same reason. The plan says "CI must pass before merge" and lists the gates; it does not require
them to run on GitHub.

## Decision

There is no hosted CI and no GitHub Actions workflow. In this repository "CI" means `just ci`:

- `just ci` on the committed HEAD, on the machine the change was made on, is the merge gate.
  `bin/ci-stamp.sh` records a pass on a clean tree; the `pr-gate` hook refuses `gh pr create`
  without one; the PR body quotes the summary lines and names the OS.
- `just secrets-scan history` runs before every push.
- `just app-build` builds the Tauri shell for the local OS; a change to `apps/app` runs it. macOS
  and Windows builds, and jobs of about an hour or more, run on a machine or VM the owner provides
  when a change needs them. An agent asks first and reports machine, commit, what ran and the
  result.
- The GitHub ruleset on `main` and `next` keeps PR-only, squash-only, no force push and no
  deletion, and requires no status checks.

## Consequences

- No Actions minutes are spent. Nothing on GitHub checks a PR, so a reviewer who wants proof runs
  `just ci` on the PR's head in a worktree; the ticket-reviewer goal does this.
- Dependabot keeps opening update PRs (the plan's supply-chain list); each is checked out and run
  through `just ci` locally before merging.
- Releases are cut by a local `just release` recipe instead of a tag-triggered workflow
  (`docs/specs/m0/01-repo-and-ci.md`).
- The plan's "Tauri builds in CI for every platform" and the macOS and iOS simulator smoke tests
  become runs on the owner's machines, scheduled with the M5 work.
- Revisit if the repository becomes public (Actions minutes are free there) or the owner sets up a
  self-hosted runner.
