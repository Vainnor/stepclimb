# Repository and CI

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/technical.md` -> Architecture and stack (layout), Testing, How it's built,
  Security and privacy (supply chain), Hosting and operations (Releases, Images), License and
  governance, Milestones (M0: "Repo, CI, releases"), Decided (Repo setup);
  `docs/adr/0018-repository-and-ci.md`
- **Depends on:** none
- **Review:** standard

## Summary

The repository skeleton, its toolchains, the `just ci` merge gate, the GitHub Actions workflows,
Dependabot and the git hooks already exist; this spec records what they do (checked against the
files on 12 Nov 2026) so later changes have a baseline. It then specifies what M0 still needs
for "Repo, CI, releases": the GitHub repository itself, the governance files the plan names, the
`pnpm audit` gate, Dependabot for the Compose images, one version number with a check, and a
release workflow. What a release ships beyond a tagged GitHub Release is pending Q1.

## Behavior

### What exists

- **B1. Workspaces and toolchains.** One Cargo workspace (`Cargo.toml`, `resolver = "3"`,
  edition 2024, 11 member crates under `crates/`) with every shared dependency pinned exactly in
  `[workspace.dependencies]`; workspace lints forbid `unsafe_code` and deny `dbg_macro`, `todo`
  and `unimplemented`. Rust is pinned to 1.98 in `rust-toolchain.toml` (with `rustfmt` and
  `clippy`). The Tauri shell `apps/app/src-tauri` is its own Cargo workspace, excluded from the
  root one. One pnpm workspace (`pnpm-workspace.yaml`: `web`, `apps/app`, `apps/ui-docs`,
  `packages/*`) with `saveExact: true` and `minimumReleaseAge: 20160` (14 days, in minutes); Node
  26.13.0 (`.node-version`, `engines`) and pnpm 12.9.0 (`packageManager`). `.cargo/config.toml`
  sets `SQLX_OFFLINE=true`, so builds read the committed `.sqlx/`. `flake.nix` provides a Nix dev
  shell with the same tools; nothing in the build reads it.
- **B2. The merge gate.** `just ci` runs, in this order: `secrets-scan` (gitleaks over the tree,
  `.gitleaks.toml`), `fmt-check`, `lint` (clippy `-D warnings`), `layering`
  (`bin/check-layering.sh`), `spdx-check` (`bin/check-spdx.sh`), `up` (PostgreSQL 16 and MinIO in
  Compose), `test-rust` (web build, `cargo nextest run --workspace`, doctests), `deny`
  (`cargo deny check`, `deny.toml`), `sqlx-check`, `openapi-diff` (the committed
  `openapi/stepclimb.json` and `packages/api-client/src/schema.ts` against what the code
  generates), `web-lint` (Biome), `typecheck` (tsc in every package), `test-web` (Vitest),
  `e2e` (Playwright against the Compose stack, image built from `deploy/Dockerfile`) and
  `agents-check`. Its last step, `bin/ci-stamp.sh`, records the HEAD commit as passed only when
  the working tree is clean; `.claude/hooks/pr-gate.sh` reads that stamp before `gh pr create`.
- **B3. Gates by layer.** The plan's gates (How it's built) map to recipes: formatting (`fmt-check`,
  `web-lint`), lints (`lint`, `web-lint`), unit, property and integration tests (`test-rust`,
  `test-web`), the four-case permission and cross-organization tests (in `test-rust`, permission
  suite from `09-permissions-and-roles.md`), the sqlx offline check (`sqlx-check`), the OpenAPI diff
  (`openapi-diff`), Playwright (`e2e`), secret scanning (`secrets-scan`). The activity known-case
  library joins `test-rust` in M3; property tests arrive with `stepclimb-assess` in M2.
- **B4. `ci.yml`.** Runs on every pull request and on pushes to `main` and `next`, on
  `ubuntu-24.04`, 60-minute timeout, `permissions: contents: read`, one run per ref
  (`cancel-in-progress`). It checks out the full history, installs the toolchain from
  `rust-toolchain.toml`, installs `just` 1.59.0, `cargo-nextest` 0.9.149, `cargo-deny` 0.20.2,
  `sqlx-cli` 0.9.0 and `gitleaks` 8.30.1, sets up pnpm and Node from `.node-version`, installs
  packages and Playwright's Chromium, scans the git history for secrets
  (`just secrets-scan history`), then runs `just ci` with `NEXTEST_PROFILE=ci`. Every action is
  pinned by commit SHA.
- **B5. `tauri.yml`.** Builds `apps/app` unsigned and unbundled: on Linux for every pull request
  and push to `main` or `next`; on `macos-15` and `windows-2025` only for pushes to `main`
  (owner, 2026-11-12, to save the private repository's Actions minutes). On Linux it also runs
  `cargo deny` over the app's own workspace. Standard hosted runners only.
- **B6. Dependabot** (`.github/dependabot.yml`). Weekly Cargo updates for `/` and
  `/apps/app/src-tauri`, weekly npm updates for `/`, monthly GitHub Actions and Docker
  (`/deploy`) updates; every entry targets `next`, holds versions for 14 days (`cooldown`) and
  groups minor and patch updates for Cargo and npm. Commit prefixes are `chore(deps)` and
  `ci(deps)`.
- **B7. Git hooks** (`.githooks/`, installed once per clone by `just install-hooks`):
  `pre-commit` scans staged changes for secrets, checks Rust formatting when a `.rs` file is
  staged and runs Biome on staged TypeScript, JavaScript, JSON and CSS; `commit-msg` refuses AI
  attribution (`AGENTS.md` non-negotiable 1), sharing patterns with
  `.claude/hooks/attribution-gate.sh`.
- **B8. Branch model.** `main` holds released code and `next` is the integration branch:
  feature branches come from `next`, PRs target `next`, squash merges only, and `next` is
  promoted to `main` by a release PR. Branch names are `{type}/{issue-or-hash}/{short-desc}`;
  commits follow Conventional Commits with the scopes in `AGENTS.md`. `.claude/hooks/git-guard.sh`
  refuses pushes to `main` or `next` and commits while on either.
- **B9. State on 13 Nov 2026.** `Vainnor/stepclimb` exists (private, squash-only, `next` the
  default branch) with the owner's bootstrap commit on `main` and `next`; ADR 0018 records the
  ruleset that protects both. The skeleton arrives in three PRs against `next`. `SECURITY.md`, `CONTRIBUTING.md` and `CODE_OF_CONDUCT.md` exist as
  drafts from the skeleton: `SECURITY.md` asks for private reports to the maintainer (GitHub's
  private vulnerability reporting answers 404 on a private repository, checked 12 Nov 2026), `CONTRIBUTING.md` leaves how the DCO is certified to Q5, and
  `CODE_OF_CONDUCT.md` is the Contributor Covenant 2.1 with no contact address yet (Q6).
  The workspace version is `0.0.0` (`Cargo.toml`), as is `apps/app/src-tauri/tauri.conf.json`;
  the npm packages carry no `version`. `stepclimb --version` prints the Cargo version (clap's
  `version` attribute in `crates/stepclimb-server/src/main.rs`).

### What M0 adds

- **B10. The GitHub repository.** The owner creates the private repository named in ADR 0018, makes
  the first commit on `next` (the owner's, since `git-guard` refuses agent commits on it), creates
  `main` from it, sets `next` as the default branch for PRs, and turns on the branch rules GitHub
  offers on the account's plan: PR required, the `ci / ci` and `tauri / build (ubuntu-24.04)` checks
  required, squash merge only, no force push. Rules it cannot turn on are recorded as missing, and
  the result goes in ADR 0018's Consequences. The GitHub Project board is created then too.
- **B11. Governance files.** `SECURITY.md` says how to report a vulnerability privately (channel
  pending Q6), which versions get fixes (the plan: security fixes are backported to the previous
  minor release for 6 months, from 1.0), and what a reporter can expect. `CONTRIBUTING.md` points
  at `AGENTS.md` for the workflow and gates, states the DCO (pending Q5 for how it is checked),
  the license split (AGPL-3.0-or-later; Apache-2.0 for `packages/api-client`) and that
  contributions come through pull requests against `next`. `CODE_OF_CONDUCT.md` holds the text
  pending Q6. `bin/check-spdx.sh` does not cover Markdown, so these files need no header.
- **B12. `pnpm audit` gate.** The plan's supply-chain list names `pnpm audit`; `just ci` does not
  run it today. A recipe `web-audit` runs `pnpm audit --prod` and `pnpm audit` (dev dependencies
  too) at the severity threshold pending Q7, and fails on a finding at or above it. It needs the
  npm registry, so it runs in `ci.yml` as its own step before `just ci`, and in `just ci` only
  when the registry answers (pending Q7 for whether a local run without the network fails or
  skips with a message).
- **B13. Dependabot for the Compose images.** `deploy/compose/compose.yaml` pins `postgres` and
  `minio` by digest, and the `docker` entry for `/deploy` reads only `deploy/Dockerfile`. A
  `docker-compose` entry for `/deploy/compose` (monthly, `next`, 14-day cooldown) keeps those
  digests current; the issue first checks that Dependabot's `docker-compose` ecosystem reads
  digest pins, and records the result in the PR. The MinIO image choice itself is ADR 0019
  (Proposed).
- **B14. One version.** Semantic versioning (Hosting and operations -> Releases). One version
  number covers the server, the web app and the Tauri app: `[workspace.package] version` in
  `Cargo.toml` is the source, and `apps/app/src-tauri/tauri.conf.json` and
  `apps/app/src-tauri/Cargo.toml` carry the same value. `bin/check-versions.sh`, run by a
  `version-check` recipe in `just ci`, fails when they differ. A `release-version <x.y.z>` recipe
  writes the value to every file and refreshes both lockfiles; its result is its own commit
  (lockfile and version bumps go in their own commit, `AGENTS.md`). The numbering before 1.0 is
  pending Q2.
- **B15. Release process.** A release is a PR from `next` to `main` titled
  `chore(release): vX.Y.Z`, carrying the version bump (B14) and the release notes. After it is
  merged (merge method pending Q3), the owner tags the merge result on `main` as `vX.Y.Z` (an
  annotated tag). Pushing a `v*` tag runs `.github/workflows/release.yml`, which checks that the
  tag equals the workspace version and that `just ci` passed on that commit's `ci.yml` run, then
  creates the GitHub Release with the notes. Release notes are generated from the Conventional
  Commit subjects since the previous tag, grouped by type, with `feat` and `fix` first (tool
  pending Q4); breaking changes (a `!` after the type, or a `BREAKING CHANGE:` footer) are listed
  first under their own heading.
- **B16. Release artifacts.** What the release workflow publishes besides the GitHub Release is
  pending Q1. Whatever Q1 decides, artifacts are built only by `release.yml` from the tagged
  commit, never uploaded from a workstation.
- **B17. DCO.** The plan says contributions are under the DCO (License and governance). How it is
  checked, and how a `Signed-off-by` trailer fits `AGENTS.md`'s "`Closes #N` is the only trailer"
  rule and the `commit-msg` hook, is pending Q5. Until Q5 is decided no DCO check runs.

## API

None. This spec adds no endpoint. It keeps `openapi-diff` in the gate (B2), which every endpoint
spec relies on.

## Data

None. It keeps `sqlx-check` in the gate and the migration naming the other specs use
(`migrations/<timestamp>_<name>.sql`, forward-only, embedded by `stepclimb_core::db::MIGRATOR`).

## Permissions

None.

## Test cases

- **T1** (B2). `just ci` on a clean checkout of `next` passes on x86_64 Linux and prints
  `just ci: all gates passed`; the PR states the OS and every gate's summary line.
- **T2** (B2). Mutation, once per new gate: a deliberately broken input (an unformatted file, a
  stale `openapi/stepclimb.json`, a file without its SPDX header, a mismatched version) makes
  that gate fail with a message naming the file and the fix.
- **T3** (B10). A test PR against `next` that fails `ci` cannot be merged through the GitHub UI
  (or ADR 0018 records that GitHub does not enforce it on this plan).
- **T4** (B12). Adding a package version with a known advisory at or above the threshold in a
  scratch branch makes `web-audit` fail naming the advisory; removing it passes.
- **T5** (B13). After the `docker-compose` entry lands, Dependabot's update log for
  `/deploy/compose` lists the `postgres` image (checked in the repository's Insights after its
  first run, recorded in the issue).
- **T6** (B14). Changing only `tauri.conf.json`'s version makes `just version-check` fail naming
  both values; `just release-version 0.1.0` makes every file agree and `just ci` pass.
- **T7** (B15). In a fork or a scratch repository, pushing tag `v0.0.1-test.1` on a commit whose
  workspace version is `0.0.1-test.1` creates a GitHub Release with grouped notes; a tag
  that differs from the workspace version fails the workflow before it creates anything.
- **T8** (B11). `docs-reviewer` reviews the three governance files against `anti-ai-slop.md` and
  the plan's License and governance section.

## Out of scope

- Signed desktop builds with `tauri-action`, auto-update, TestFlight and the App Store: M5
  (`docs/plan/technical.md` -> Web app and native apps).
- The Tauri smoke tests on macOS and the iOS simulator: M5 (Testing -> Apps).
- The Helm chart and the k6 performance budgets: 1.0 (Milestones).
- Backport branches for security fixes: the policy starts with 1.0 (Hosting and operations ->
  Releases); their naming is decided then.
- The activity known-case library gate: M3.
- The Compose stack itself (`deploy/compose/compose.yaml`, `deploy/Dockerfile`):
  `14-compose-deploy.md`.
- A paid or larger GitHub runner: the owner's call (ADR 0018).

## Open questions

- **Q1. What an M0 release publishes, and when the plan's image and binary requirements apply.**
  The plan lists images "multi-arch on GHCR, signed with cosign, SBOM attached, distroless and
  non-root", a static musl binary for amd64 and arm64 with a systemd unit, and "signed releases"
  in the supply-chain list, but the Milestones table names none of them; M0 says only "releases".
  The pilot (M4) runs on the maintainer's Kubernetes, which needs an image in a registry. Options:
  A, M0 releases are tags and GitHub Releases with notes only; images and binaries start with 1.0
  (the reading that groups them with the Helm chart); B, M0 also pushes the `deploy/Dockerfile`
  image to GHCR for amd64 and arm64, tagged with the version, with a cosign keyless signature and
  an SBOM, leaving musl binaries and the systemd unit to 1.0; C, all of the plan's artifacts from
  M0. Recommendation: B, because the pilot needs a pulled image before 1.0, and signing and the
  SBOM are cheaper to add before anyone depends on unsigned images.
- **Q2. Version numbers before 1.0.** Options: A, `0.<milestone>.<patch>` (M0 ends at `0.1.0`, M1
  at `0.2.0`, and so on to `1.0.0`); B, a minor release whenever `next` is promoted, regardless of
  milestone; C, no releases before 1.0 except pre-release tags (`1.0.0-alpha.N`). Recommendation:
  A, so a version names the milestone it finished.
- **Q3. How a release PR is merged.** The branch model squashes every PR and promotes `next` to
  `main` with a release PR. A squash merge into `main` makes a commit `next` does not have, so the
  next release PR conflicts or carries history twice. Options: A, release PRs (only) use a merge
  commit, so `main`'s history contains `next`'s; B, fast-forward `main` to `next` (no merge
  commit; GitHub's UI cannot do it, so the owner pushes with `git push origin next:main` under a
  documented `git-guard` exception); C, squash, then merge `main` back into `next` after each
  release. Recommendation: A, with the repository setting that allows merge commits limited to PRs
  into `main` if GitHub offers it, else stated in `CONTRIBUTING.md`.
- **Q4. The release-notes tool.** Options: A, `git-cliff` (reads Conventional Commits, a single
  binary, in nixpkgs), configured in `cliff.toml`; B, `release-please`, which opens its own release
  PRs and expects to own the version bump; C, a short script in `bin/` over `git log`.
  Recommendation: A, run by the `release-version` recipe and by `release.yml`, after
  `dependency-researcher` checks it.
- **Q5. DCO enforcement and the trailer rule.** `AGENTS.md` says `Closes #N` is the only trailer
  and the `commit-msg` hook refuses attribution lines; the DCO is usually shown with a
  `Signed-off-by` trailer. Options: A, require `Signed-off-by` on PR commits from outside
  contributors only, checked by a workflow step, and allow that trailer in `AGENTS.md` and the
  hook; B, a CONTRIBUTING statement that opening a PR certifies the DCO, with no trailer and no
  check; C, the DCO GitHub App on every PR, including the maintainer's (the trailer then appears on
  every commit). Recommendation: A, since the plan expects few outside contributors and the
  maintainer's own commits stay as they are.
- **Q6. The disclosure channel and the code of conduct text.** `SECURITY.md` needs a private
  channel: A, GitHub private vulnerability reporting on the repository (it needs the repository to
  be public, or GitHub's support for private ones confirmed when it is created); B, an email
  address the owner provides. `CODE_OF_CONDUCT.md`: A, the Contributor Covenant 2.1 with the
  owner's contact; B, a short text of the owner's own. Recommendation: channel A with B as the
  fallback while the repository is private; Contributor Covenant 2.1.
- **Q7. The `pnpm audit` threshold and offline behavior.** Options: A, fail on `high` and
  `critical`, and skip with a printed warning when the registry cannot be reached locally (CI
  always reaches it); B, fail on any severity; C, fail on `moderate` and above. Recommendation: A,
  with a reviewed allowlist file for advisories that do not apply.

## Issues

1. **Create the GitHub repository and record its rules** (owner). Deliver the private repository,
   the first commit on `next`, `main`, the branch rules or the reason they are missing, the
   Project board, and the result in ADR 0018's Consequences (an agent drafts the ADR text). Covers
   B10, T3. Depends on: none.
2. **Governance files.** The skeleton drafted `SECURITY.md`, `CONTRIBUTING.md` and
   `CODE_OF_CONDUCT.md` (B9). Deliver what Q5 and Q6 decide: the DCO mechanism, the contact
   address, and private vulnerability reporting once the repository is public. Covers B11, T8. Depends on: issue 1 (the repository URL).
3. **`pnpm audit` gate** (threshold pending Q7). Deliver the `web-audit` recipe, its place in
   `just ci` and `ci.yml`, and the allowlist file if Q7 adopts one. Covers B12, T2, T4. Depends on:
   none.
4. **Dependabot for the Compose images.** Deliver the `docker-compose` entry after confirming it
   reads digest pins. Covers B13, T5. Depends on: issue 1 (Dependabot runs only on GitHub).
5. **One version and its check.** Deliver `bin/check-versions.sh` (with an SPDX header), the
   `version-check` recipe in `just ci`, and `release-version <x.y.z>`. Covers B14, T2, T6.
   Depends on: none. The starting number waits for Q2.
6. **Release workflow and notes** (tool pending Q4, merge method pending Q3). Deliver
   `.github/workflows/release.yml` (tag check, CI check, GitHub Release with grouped notes),
   the notes configuration, and the release steps in `CONTRIBUTING.md`. Covers B15, T7. Depends
   on: issues 1, 2 and 5.
7. **Release artifacts** (pending Q1). Deliver the artifacts Q1 chooses in `release.yml` (for
   option B: multi-arch image to GHCR, cosign keyless signature, SBOM attached to the release).
   Covers B16. Depends on: issue 6; `14-compose-deploy.md` for the Dockerfile.
8. **DCO check** (pending Q5). Deliver the check Q5 chooses and the matching change to `AGENTS.md`
   and `.githooks/commit-msg` if a trailer is allowed. Covers B17. Depends on: issue 2.
