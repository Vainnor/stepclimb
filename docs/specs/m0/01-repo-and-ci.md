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

The repository skeleton, its toolchains, the `just ci` merge gate (run locally; there is no hosted
CI, ADR 0021),
The git hooks already exist; this spec records what they do (checked against the
files on 12 Nov 2026) so later changes have a baseline. It then specifies what M0 still needs
for "Repo, CI, releases": the GitHub repository itself, the governance files the plan names, the
`pnpm audit` gate, the monthly dependency update, one version number with a check, and a
release recipe. What a release ships beyond a tagged GitHub Release is pending Q1.

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
  (`bin/check-layering.sh`), `lint-escapes` (`bin/check-lint-escapes.py`), `spdx-check`
  (`bin/check-spdx.sh`), `version-check` (B14), `gate-check` (the Python tests in `bin/`:
  `bin/ci_gate.py`, the version scripts, the update's rules), `test-rust` (web build,
  `cargo nextest run --workspace`, doctests), `deny` (`cargo deny check`, `deny.toml`),
  `sqlx-check`, `openapi-diff` (the committed `openapi/stepclimb.json` and
  `packages/api-client/src/schema.ts` against what the code generates), `web-lint` (Biome),
  `web-audit` (B12), `typecheck` (tsc in every package), `test-web` (Vitest), `ui-visual` (story screenshots and
  axe), `e2e` (Playwright against the Compose stack, image built from `deploy/Dockerfile`),
  `compose-check` (the production Compose example), `compose-ops-check` (the same with Caddy and
  the dumps), `image-check` (`bin/check-image.sh`) and `agents-check`. `bin/ci_gate.py` runs them with the run's own PostgreSQL and
  Compose projects, named from the worktree path and removed when the run ends, and lets at most
  `SC_CI_SLOTS` (default 3) runs share the machine, so several worktrees can gate at once.
  `just ci`'s last step, `bin/ci-stamp.sh`, records the commit checked out when the run started
  as passed, only if HEAD is still that commit and the working tree was clean at the start and is
  at the end; `.claude/hooks/pr-gate.sh` reads that stamp before `gh pr create`.
- **B3. Gates by layer.** The plan's gates (How it's built) map to recipes: formatting (`fmt-check`,
  `web-lint`), lints (`lint`, `web-lint`), unit, property and integration tests (`test-rust`,
  `test-web`), the four-case permission and cross-organization tests (in `test-rust`, permission
  suite from `09-permissions-and-roles.md`), the sqlx offline check (`sqlx-check`), the OpenAPI diff
  (`openapi-diff`), Playwright (`e2e`), secret scanning (`secrets-scan`). The activity known-case
  library joins `test-rust` in M3; property tests arrive with `stepclimb-assess` in M2.
- **B4. No hosted CI** (ADR 0021). The repository has no GitHub Actions workflows: Actions
  minutes on a private repository run out too fast. `just ci` on the committed HEAD, on the
  machine the change was made on, is the merge gate; `bin/ci-stamp.sh` records a pass on a clean
  tree and the `pr-gate` hook refuses `gh pr create` without one. The PR body quotes the summary
  lines and names the OS. The history secret scan (`just secrets-scan history`) runs before every
  push, in place of a hosted scan.
- **B5. `just app-build`.** Builds `apps/app` unsigned and unbundled for the local OS, after
  `cargo deny` over the app's own workspace. It is not part of `just ci`; a change to `apps/app`
  runs it. macOS and Windows builds run on a machine or VM the owner provides, when a change
  needs them.
- **B6. Dependency updates by hand.** There is no Dependabot (owner, 13 Nov 2026): dependencies
  are updated once a month and before each release, as in Mainline (`AGENTS.md`, Git workflow),
  in a lockfile-only PR checked with `just ci`, `just deny` and `just app-build`. Versions stay
  at least 14 days old (pnpm's `minimumReleaseAge`; the crates.io release date for crates).
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
  decided in Q6. `bin/check-spdx.sh` does not cover Markdown, so these files need no header.
- **B12. `pnpm audit` gate.** The plan's supply-chain list names `pnpm audit`; `just ci` does not
  run it today. A recipe `web-audit` runs `pnpm audit --prod` and `pnpm audit` (dev dependencies
  too) at the severity threshold pending Q7, and fails on a finding at or above it. It needs the
  npm registry, so it runs in `just ci` only when the registry answers (pending Q7 for whether a local run without the network fails or
  skips with a message).
- **B13. Monthly update recipe.** `just update-deps` runs `cargo update` in both Cargo
  workspaces and `pnpm update` within the pinned ranges, age-checks every crate version in both
  lockfiles (D3), moves each base and Compose image pin to its tag's current digest once that
  build is at least 14 days old, and prints what changed, so the monthly PR is one command. A
  registry lists only a tag's current digest, not its earlier ones, so a tag rebuilt more often
  than every 14 days keeps its old pin; each run prints how many days old every pinned build is,
  and that number grows from run to run, so a base image stuck behind its tag shows up (D4). The
  MinIO image choice itself is ADR 0019.
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
  annotated tag). The owner then runs `just release vX.Y.Z` in a clean worktree of the tag: it
  checks that the tag equals the workspace version and that `just ci` passes on that commit, then
  creates the GitHub Release with `gh release create` and the notes. Release notes are generated from the Conventional
  Commit subjects since the previous tag, grouped by type, with `feat` and `fix` first (tool
  pending Q4); breaking changes (a `!` after the type, or a `BREAKING CHANGE:` footer) are listed
  first under their own heading.
- **B16. Release artifacts.** What a release publishes besides the GitHub Release is pending Q1.
  Whatever Q1 decides, artifacts are built by `just release` from a clean worktree of the tagged
  commit, on the owner's machine or a VM the owner provides, never from a working checkout.
- **B17. DCO** (Q5, decided). Opening a pull request certifies the DCO: `CONTRIBUTING.md` says
  so and the PR template has a certification box. No `Signed-off-by` trailer is required, so
  `Closes #N` stays the only trailer and the `commit-msg` hook is unchanged.

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
- **T5** (B13). On a branch, `just update-deps` followed by `just ci` leaves a lockfile and
  digest diff only (no change to any manifest's pinned versions) and the gate green.
- **T6** (B14). Changing only `tauri.conf.json`'s version makes `just version-check` fail naming
  both values; `just release-version 0.1.0` makes every file agree and `just ci` pass.
- **T7** (B15). In a scratch repository, `just release v0.0.1-test.1` on a tagged commit whose
  workspace version is `0.0.1-test.1` creates a GitHub Release with grouped notes; a tag
  that differs from the workspace version makes `just release` stop before it creates anything.
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

The owner accepted every recommendation below on 13 Nov 2026. Where a behavior or an issue
says "pending Q<n>" or "Q<n> answered", build the recommended option; the owner can still
overrule one, which then updates this spec.

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
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q2. Version numbers before 1.0.** Options: A, `0.<milestone>.<patch>` (M0 ends at `0.1.0`, M1
  at `0.2.0`, and so on to `1.0.0`); B, a minor release whenever `next` is promoted, regardless of
  milestone; C, no releases before 1.0 except pre-release tags (`1.0.0-alpha.N`). Recommendation:
  A, so a version names the milestone it finished.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q3. How a release PR is merged.** The branch model squashes every PR and promotes `next` to
  `main` with a release PR. A squash merge into `main` makes a commit `next` does not have, so the
  next release PR conflicts or carries history twice. Options: A, release PRs (only) use a merge
  commit, so `main`'s history contains `next`'s; B, fast-forward `main` to `next` (no merge
  commit; GitHub's UI cannot do it, so the owner pushes with `git push origin next:main` under a
  documented `git-guard` exception); C, squash, then merge `main` back into `next` after each
  release. Recommendation: A, with the repository setting that allows merge commits limited to PRs
  into `main` if GitHub offers it, else stated in `CONTRIBUTING.md`.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q4. The release-notes tool.** Options: A, `git-cliff` (reads Conventional Commits, a single
  binary, in nixpkgs), configured in `cliff.toml`; B, `release-please`, which opens its own release
  PRs and expects to own the version bump; C, a short script in `bin/` over `git log`.
  Recommendation: A, run by the `release-version` recipe and by `just release`, after
  `dependency-researcher` checks it.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q5. DCO enforcement and the trailer rule.** Decided by the owner on 13 Nov 2026 (left to the
  agent's choice): option B, opening a PR certifies the DCO, with a box in the PR template and no
  trailer or check. Option A (a `Signed-off-by` trailer for outside contributors) was the earlier
  recommendation; B keeps the "only trailer" rule without an exception.
- **Q6. The disclosure channel and the code of conduct text.** Decided by the owner on 13 Nov
  2026: reports go to contact@vainnor.com, plus GitHub private vulnerability reporting once the
  repository is public; the code of conduct is the Contributor Covenant 2.1 with that address.
- **Q7. The `pnpm audit` threshold and offline behavior.** Options: A, fail on `high` and
  `critical`, and skip with a printed warning when the registry cannot be reached; B, fail on any severity; C, fail on `moderate` and above. Recommendation: A,
  with a reviewed allowlist file for advisories that do not apply.
  **Decided:** the recommendation (owner, 13 Nov 2026).

### Coordinator decisions (2026-11-14)

Points the behaviors above left open, settled while building issues 2 to 5 on the owner's
delegation; the owner can overrule any of them.

- **D1. When the registry "answers" (B12, Q7).** Before the audit, `bin/check-pnpm-audit.mjs`
  asks the configured registry's bulk advisory endpoint about `lodash` 4.17.20, which has public
  high advisories, with a 15 s timeout. Only a network error or the timeout makes `web-audit`
  print `SKIPPED` and pass; it then writes `target/web-audit-skipped`, and `just ci`'s last line
  says `web-audit SKIPPED (offline)`. An answer without lodash's advisories fails the gate, since
  an empty report from that endpoint cannot be trusted, and so does any later `pnpm audit` error.
  Node's `fetch` uses a proxy only from `HTTPS_PROXY` or `HTTP_PROXY` with `NODE_USE_ENV_PROXY=1`,
  which the recipe sets; a proxy set only in pnpm's configuration fails the gate, naming the
  variable to export.
- **D2. The allowlist (B12, Q7).** `pnpm-audit-allowlist.json` lists advisories by GHSA id, each
  with `package` (the advisory's package name), `reason` and `expires`, a real calendar date at
  most 90 days ahead. Days are UTC. An entry passes through its expiry date and fails the day
  after. An entry fails when its advisory is gone or now below `high`, when its package is not
  the advisory's, or when a field is missing or duplicated. Below `high`, advisories are printed
  and pass without an entry; an advisory with a severity outside info, low, moderate, high and
  critical, or with no GHSA id, fails. The list
  starts empty: on 14 Nov 2026 `pnpm audit` reports 2 moderate advisories (glob
  GHSA-3g3m-2fjr-6pw4, minimatch GHSA-g3hv-75wg-m249, both under Storybook) and none higher.
- **D3. The 14-day rule for crates (B6, B13).** Stable Cargo 1.98 has no minimum release age
  (`-Z min-publish-age` is nightly-only). After `cargo update`, `bin/update_deps.py` looks up
  every crate version in both lockfiles on the crates.io API, the ones the lockfile already had
  too. One published less than 14 days ago is set back with
  `cargo update -p name@version --precise` to the newest unyanked release of the same semver
  series at least 14 days old, one crate at a time, reading the lockfile again after each. When
  Cargo refuses (a dependent requires the newer version) or the series has nothing old enough,
  the run fails naming the crate and Cargo's reason.
- **D4. Image age (B13).** An image's age is the linux/amd64 build date (`created`) of a digest.
  The pin moves to the tag's current digest only when that build is at least 14 days old. A newer
  build, or a placeholder date (reproducible images such as distroless report 1970-01-01), keeps
  the old pin, and the run lists it with the pinned build's age. The images are those in `deploy/Dockerfile` and
  `deploy/compose/`; the Playwright image in `bin/ui-visual.sh` pins the amd64 manifest the
  story screenshot baselines were drawn with, so it moves only with them and `update-deps`
  leaves it alone.
- **D5. What "one version" covers (B14).** Besides the three files B14 names, `version-check`
  reads every workspace member `cargo metadata --no-deps` lists (each must say
  `version.workspace = true`), the workspace
  crates' entries in both Cargo lockfiles, and any `package.json` that carries a `version`
  outside Changesets. `packages/ui` and `packages/tokens` keep the versions Changesets gives them
  (ADR 0015); the other npm packages carry none today.
- **D6. Private vulnerability reporting (B11, issue 2).** Q6's email address is the channel.
  GitHub's private vulnerability reporting stays off while the repository is private; when it
  goes public, the owner switches it on in the repository settings (Security, "Private
  vulnerability reporting") and adds it to `SECURITY.md`. No file in the repository can turn it
  on.

## Issues

1. **Create the GitHub repository and record its rules** (owner). Deliver the private repository,
   the first commit on `next`, `main`, the branch rules or the reason they are missing, the
   Project board, and the result in ADR 0018's Consequences (an agent drafts the ADR text). Covers
   B10, T3. Depends on: none.
2. **Governance files.** The skeleton drafted `SECURITY.md`, `CONTRIBUTING.md` and
   `CODE_OF_CONDUCT.md` (B9), and Q5 and Q6 are decided and in them. Left: turn on GitHub private
   vulnerability reporting and mention it in `SECURITY.md` when the repository goes public.
   Covers B11, T8. Depends on: issue 1.
3. **`pnpm audit` gate** (threshold pending Q7). Deliver the `web-audit` recipe, its place in
   `just ci`, and the allowlist file if Q7 adopts one. Covers B12, T2, T4. Depends on: none.
4. **Monthly update recipe.** Deliver `just update-deps` and the monthly steps in
   `CONTRIBUTING.md`. Covers B13, T5. Depends on: none.
5. **One version and its check.** Deliver `bin/check-versions.sh` (with an SPDX header), the
   `version-check` recipe in `just ci`, and `release-version <x.y.z>`. Covers B14, T2, T6.
   Depends on: none. The starting number waits for Q2.
6. **Release recipe and notes** (tool pending Q4, merge method pending Q3). Deliver
   `just release vX.Y.Z` (tag check, `just ci` on the tagged commit, GitHub Release with grouped
   notes through `gh`), the notes configuration, and the release steps in `CONTRIBUTING.md`.
   Covers B15, T7. Depends on: issues 1, 2 and 5.
7. **Release artifacts** (pending Q1). Deliver the artifacts Q1 chooses in `just release` (for
   option B: multi-arch image to GHCR, cosign signature, SBOM attached to the release), built on
   the owner's machine or VM. Covers B16. Depends on: issue 6; `14-compose-deploy.md` for the
   Dockerfile.
8. **DCO certification** (Q5, decided: option B). Delivered with the docs PR: the
   `CONTRIBUTING.md` text and the PR template box. Covers B17. Depends on: issue 2.